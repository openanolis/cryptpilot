#[cfg(feature = "provider-kbs")]
pub mod kbs;
#[cfg(feature = "provider-kms")]
pub mod kms;
#[cfg(feature = "provider-oidc")]
pub mod oidc;
#[cfg(feature = "provider-otp")]
pub mod otp;
#[cfg(feature = "provider-tpm2")]
pub mod tpm2;
#[cfg(feature = "provider-exec")]
pub mod exec;

use anyhow::Result;

use crate::types::Passphrase;

pub trait KeyProvider {
    #[allow(async_fn_in_trait)]
    async fn get_key(&self) -> Result<Passphrase>;

    fn volume_type(&self) -> VolumeType;
}

pub trait IntoProvider {
    type Provider: KeyProvider;

    fn into_provider(self) -> Self::Provider;
}

pub enum VolumeType {
    /// Temporary volume, which will drop all the data after closing.
    Temporary,

    /// Persistent volume, which will keep the data after closing, and can be opened again with the same passphrase.
    Persistent,
}

#[cfg(test)]
pub mod tests {

    use std::{future::Future, path::PathBuf};

    use crate::{
        async_defer,
        cli::{CloseOptions, InitOptions, OpenOptions},
        cmd::{close::CloseCommand, init::InitCommand, open::OpenCommand, Command as _},
        config::{
            volume::{MakeFsType, VolumeConfig},
            ConfigBundle,
        },
        fs::{block::dummy::DummyDevice, cmd::CheckCommandOutput as _},
        provider::{IntoProvider, KeyProvider, VolumeType},
    };

    use anyhow::{Context, Result};
    use block_devs::BlckExt as _;
    use tokio::{
        fs::File,
        io::{AsyncReadExt, AsyncWriteExt},
        process::Command,
    };
    use tokio_util::bytes::BytesMut;

    use rstest_reuse::template;

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
                volume: volume_config.volume.clone(),
            },
        }
        .run()
        .await?;

        async_defer! {
            async{
                CloseCommand{
                    close_options: CloseOptions{
                        volume: volume_config.volume.clone(),
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
        open_then(&volume_config, |volume_config| async move {
            let mount_dir = tempfile::Builder::new()
                .prefix("cryptpilot-mount-")
                .tempdir()?;

            Command::new("mount")
                .arg(volume_config.volume_path())
                .arg(mount_dir.path())
                .run_check_output()
                .await?;

            async_defer! {
                async{
                    Command::new("umount")
                        .arg(mount_dir.path())
                        .run_check_output()
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
            }

            task(volume_config.clone(), mount_dir.path().into()).await
        })
        .await
    }

    pub async fn open_and_swapon<F, T>(volume_config: &VolumeConfig, task: F) -> Result<()>
    where
        F: FnOnce(VolumeConfig) -> T,
        T: Future<Output = Result<()>>,
    {
        open_then(&volume_config, |volume_config| async move {
            Command::new("swapon")
                .arg(volume_config.volume_path())
                .run_check_output()
                .await?;

            async_defer! {
                async{
                    Command::new("swapoff")
                        .arg(volume_config.volume_path())
                        .run_check_output()
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
            }

            task(volume_config.clone()).await
        })
        .await
    }

    pub async fn run_test_on_volume(config_str: &str) -> Result<()> {
        let mut volume_config: VolumeConfig = toml::from_str(config_str)?;

        // Random volume name
        volume_config.volume = format!("test-{}", rand::random::<u64>());

        let dummy_device = if volume_config.extra_config.makefs == Some(MakeFsType::Swap) {
            DummyDevice::setup_on_disk(1 * 1024 * 1024 * 1024 /* 1G */).await?
        } else {
            DummyDevice::setup_on_tmpfs(100 * 1024 * 1024 * 1024 /* 100G */).await?
        };

        volume_config.dev = dummy_device
            .path()?
            .into_os_string()
            .to_str()
            .context("Cannot convert dummy device path to str")?
            .to_owned();

        crate::config::source::set_config_source(ConfigBundle {
            global: None,
            fde: None,
            volumes: vec![volume_config.clone()],
        })
        .await;

        InitCommand {
            init_options: InitOptions {
                volume: volume_config.volume.clone(),
                force_reinit: false,
                yes: true,
            },
        }
        .run()
        .await?;

        match &volume_config.extra_config.makefs {
            Some(MakeFsType::Swap) => {
                // Open and swapon
                open_and_swapon(&volume_config, |volume_config| async move {
                    let swap_device_size = File::open(volume_config.volume_path())
                        .await?
                        .into_std()
                        .await
                        .get_block_device_size()?;

                    // Run stress-ng to consume swap memory
                    Command::new("systemd-run")
                        .arg("--wait")
                        .arg("--property=MemoryMax=128M")
                        .arg("--property=MemorySwapMax=infinity")
                        .arg("--")
                        .arg("stress-ng")
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
                        .run_check_output()
                        .await?;

                    Ok(())
                })
                .await?;
            }
            Some(_) => {
                // Open and write file
                open_and_mount(&volume_config, |_, mount_dir: PathBuf| async move {
                    let mut file = File::options()
                        .write(true)
                        .create(true)
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

                // Open again and test with pjdfstest
                open_and_mount(&volume_config, |_, mount_dir: PathBuf| async move {
                    Command::new("prove")
                        .arg("-rv")
                        .arg("/tmp/pjdfstest/tests")
                        .current_dir(mount_dir)
                        .run_check_output()
                        .await?;
                    Ok(())
                })
                .await?;
            }
            None => {
                // Just Open it and do nothing
                open_then(&volume_config, |_| async move { Ok(()) }).await?;
                // Test again
                open_then(&volume_config, |_| async move { Ok(()) }).await?;
            }
        }

        Ok(())
    }
}
