// Volume integration tests
// These test the complete volume lifecycle: init, open, use, close

#![allow(unused_macros)]

use std::{future::Future, path::PathBuf};

use cryptpilot_crypt::{
    async_defer,
    cli::{CloseOptions, InitOptions, OpenOptions},
    cmd::{close::CloseCommand, init::InitCommand, open::OpenCommand, Command as _},
    config::{
        source::{set_volume_config_source, VolumeConfigSource},
        volume::VolumeConfig,
    },
};

use cryptpilot::{
    fs::{block::dummy::DummyDevice, cmd::CheckCommandOutput as _, mount::TmpMountPoint},
    provider::{IntoProvider, KeyProvider, VolumeType},
    types::MakeFsType,
};

use anyhow::Result;
use async_trait::async_trait;
use block_devs::BlckExt as _;
use cgroups_rs::{cgroup_builder::CgroupBuilder, Cgroup, CgroupPid};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::bytes::BytesMut;

use rstest_reuse::template;

struct InMemoryVolumeConfigSource {
    volumes: Vec<VolumeConfig>,
}

#[async_trait]
impl VolumeConfigSource for InMemoryVolumeConfigSource {
    fn source_debug_string(&self) -> String {
        "in-memory test volume config".to_owned()
    }

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        Ok(self.volumes.clone())
    }
}

#[template]
#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
#[two_rusty_forks::test_fork]
pub async fn test_volume_base(
    #[values("swap", "ext4", "xfs", "vfat")] makefs: &str,
    #[values(false, true)] integrity: bool,
) -> Result<()> {
}

pub async fn open_then<F, T>(volume_config: &VolumeConfig, task: F) -> Result<()>
where
    F: FnOnce(VolumeConfig) -> T,
    T: Future<Output = Result<()>>,
{
    OpenCommand {
        open_options: OpenOptions {
            volume: vec![volume_config.volume.clone()],
            check_fs: false,
        },
    }
    .run()
    .await?;

    async_defer! {
        async{
            CloseCommand{
                close_options: CloseOptions{
                    volume: vec![volume_config.volume.clone()],
                }
            }.run().await?;
            Ok::<_, anyhow::Error>(())
        }
    }

    task(volume_config.clone()).await
}

pub async fn open_and_mount<F, T>(volume_config: &VolumeConfig, task: F) -> Result<()>
where
    F: FnOnce(VolumeConfig, PathBuf) -> T,
    T: Future<Output = Result<()>>,
{
    open_then(volume_config, |volume_config| async move {
        let tmp_mount = TmpMountPoint::mount(volume_config.volume_path(), true).await?;

        task(volume_config, tmp_mount.mount_point().to_path_buf()).await
    })
    .await
}

pub async fn open_and_swapon<F, T>(volume_config: &VolumeConfig, task: F) -> Result<()>
where
    F: FnOnce(VolumeConfig) -> T,
    T: Future<Output = Result<()>>,
{
    open_then(volume_config, |volume_config| async move {
        Command::new("swapon")
            .arg(volume_config.volume_path())
            .run()
            .await?;

        async_defer! {
            async{
                Command::new("swapoff")
                    .arg(volume_config.volume_path())
                    .run()
                    .await?;
                Ok::<_, anyhow::Error>(())
            }
        }

        task(volume_config.clone()).await
    })
    .await
}

pub async fn run_test_on_volume(config_str: &str, use_external_suite: bool) -> Result<()> {
    // Set test mode environment variable to skip external binary checks
    std::env::set_var("CRYPTPILOT_TEST_MODE", "1");

    let mut volume_config: VolumeConfig = toml::from_str(config_str)?;

    // Random volume name
    volume_config.volume = format!("test-{}", rand::random::<u64>());

    let dummy_device = if volume_config.extra_config.makefs == Some(MakeFsType::Swap) {
        DummyDevice::setup_on_cache_dir(1024 * 1024 * 1024 /* 1G */).await?
    } else {
        DummyDevice::setup_on_tmpfs(100 * 1024 * 1024 * 1024 /* 100G */).await?
    };

    volume_config.dev = dummy_device.path()?;

    set_volume_config_source(InMemoryVolumeConfigSource {
        volumes: vec![volume_config.clone()],
    })
    .await;

    InitCommand {
        init_options: InitOptions {
            volume: vec![volume_config.volume.clone()],
            force_reinit: false,
            yes: true,
        },
    }
    .run()
    .await?;

    match &volume_config.extra_config.makefs {
        Some(MakeFsType::Swap) => {
            // Just Open it and checking
            open_then(&volume_config, |volume_config| async move {
                if !matches!(volume_config.extra_config.integrity, Some(true)) {
                    assert!(
                        cryptpilot::fs::mkfs::has_valuable_data(
                            &volume_config.volume_path(),
                            volume_config.extra_config.makefs
                        )
                        .await?
                    );
                }
                Ok(())
            })
            .await?;

            // Open and swapon
            open_and_swapon(&volume_config, |volume_config| async move {
                if use_external_suite {
                    let swap_device_size = File::open(volume_config.volume_path())
                        .await?
                        .into_std()
                        .await
                        .get_block_device_size()?;

                    let hier = cgroups_rs::hierarchies::auto();
                    let cg: Cgroup =
                        CgroupBuilder::new(&format!("cryptpilot-test-{}", rand::random::<u64>()))
                            .memory()
                            .memory_hard_limit(128 * 1024 * 1024 /* 128M */)
                            .memory_swap_limit(-1 /* infinity */)
                            .done()
                            .build(hier)?;

                    let cg_clone = cg.clone();
                    async_defer! {
                        async{
                            cg_clone.delete()
                        }
                    }

                    // Run stress-ng to consume swap memory
                    Command::new("stress-ng")
                        .arg("--timeout")
                        .arg("10")
                        .arg("--vm")
                        .arg("1")
                        .arg("--vm-hang")
                        .arg("0")
                        .arg("--vm-method")
                        .arg("zero-one")
                        .arg("--vm-bytes")
                        .arg(swap_device_size.to_string())
                        .run_with_child_callback(move |pid| Ok(cg.add_task(CgroupPid::from(pid))?))
                        .await?;
                }

                Ok(())
            })
            .await?;
        }
        Some(_) => {
            // Just Open it and checking
            open_then(&volume_config, |volume_config| async move {
                if !matches!(volume_config.extra_config.integrity, Some(true)) {
                    assert!(
                        cryptpilot::fs::mkfs::has_valuable_data(
                            &volume_config.volume_path(),
                            volume_config.extra_config.makefs
                        )
                        .await?
                    );
                }
                Ok(())
            })
            .await?;

            // Open and write file
            open_and_mount(&volume_config, |_, mount_dir: PathBuf| async move {
                let mut file = File::options()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(mount_dir.join("testfile"))
                    .await?;
                file.write_all("test".as_bytes()).await?;
                file.flush().await?;
                Ok(())
            })
            .await?;

            // Open again and read file
            open_and_mount(
                &volume_config,
                |volume_config, mount_dir: PathBuf| async move {
                    match volume_config
                        .encrypt
                        .key_provider
                        .into_provider()
                        .volume_type()
                    {
                        VolumeType::Temporary => {
                            assert!(!mount_dir.join("testfile").exists())
                        }
                        VolumeType::Persistent => {
                            let mut file: File = File::options()
                                .read(true)
                                .open(mount_dir.join("testfile"))
                                .await?;
                            let mut buf = BytesMut::new();
                            file.read_buf(&mut buf).await?;
                            assert_eq!("test".as_bytes(), &buf);
                        }
                    }
                    Ok(())
                },
            )
            .await?;

            if use_external_suite {
                // Open again and test with pjdfstest
                open_and_mount(&volume_config, |_, mount_dir: PathBuf| async move {
                    Command::new("prove")
                        .arg("-rv")
                        .arg("/tmp/pjdfstest/tests")
                        .current_dir(mount_dir)
                        .run()
                        .await?;
                    Ok(())
                })
                .await?;
            }
        }
        None => {
            // Just Open it and do nothing but checking
            open_then(&volume_config, |volume_config| async move {
                // Add assertion to check if disk is empty
                assert!(
                    !cryptpilot::fs::mkfs::has_valuable_data(
                        &volume_config.volume_path(),
                        volume_config.extra_config.makefs
                    )
                    .await?
                );
                Ok(())
            })
            .await?;
            // Test again
            open_then(&volume_config, |volume_config| async move {
                // Add assertion to check if disk is still empty after re-open
                assert!(
                    !cryptpilot::fs::mkfs::has_valuable_data(
                        &volume_config.volume_path(),
                        volume_config.extra_config.makefs
                    )
                    .await?
                );
                Ok(())
            })
            .await?;
        }
    }

    Ok(())
}
