use std::path::Path;

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use block_devs::BlckExt as _;
use nix::unistd::SysconfVar;
use ordermap::OrderSet;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    process::Command,
};

use crate::{
    config::volume::MakeFsType,
    fs::{block::dummy::DummyDevice, cmd::CheckCommandOutput as _},
};

use super::block::blktrace::BlkTrace;

#[async_trait]
pub trait MakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()>;
}

pub struct NormalMakeFs;

#[async_trait]
impl MakeFs for NormalMakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()> {
        // Use mkfs commands directly instead of systemd-makefs
        let (mkfs_cmd, force_arg) = match fs_type {
            MakeFsType::Swap => ("mkswap", "-f"), // mkswap uses -f for force
            MakeFsType::Ext4 => ("mkfs.ext4", "-F"), // mkfs.ext4 uses -F for force
            MakeFsType::Xfs => ("mkfs.xfs", "-f"), // mkfs.xfs uses -f for force
            MakeFsType::Vfat => ("mkfs.vfat", "-I"), // mkfs.vfat uses -I to force formatting
        };

        Command::new(mkfs_cmd)
            .arg(force_arg)
            .arg(device_path.as_ref())
            .run()
            .await?;
        Ok(())
    }
}

pub struct IntegrityNoWipeMakeFs;

#[async_trait]
impl MakeFs for IntegrityNoWipeMakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()> {
        let device_path = device_path.as_ref();
        let is_empty_disk = {
            Command::new("file")
                .args(["-E", "--brief", "--dereference", "--special-files"])
                .arg(device_path)
                .env("LC_ALL", "C")
                .run_with_status_checker(|code, stdout, _| {
                    let stdout = String::from_utf8_lossy(&stdout);

                    let is_empty_disk =
                        if stdout.contains("Input/output error") || stdout.trim() == "data" {
                            true
                        } else if stdout.contains("cannot open") {
                            bail!("Cannot open")
                        } else if code != 0 {
                            bail!("Bad exit code")
                        } else {
                            false
                        };

                    Ok(is_empty_disk)
                })
                .await
                .context("Failed to detecting filesystem type")?
        };

        if is_empty_disk {
            tracing::debug!(
                "The device {device_path:?} is uninitialized (empty) and is ok to be initialized with mkfs"
            );
            Self::mkfs_on_no_wipe_volume(device_path, fs_type).await?
        } else {
            tracing::debug!(
                "The device {device_path:?} is not empty and maybe some data on it, so we won't touch it"
            );
        }

        Ok(())
    }
}

impl IntegrityNoWipeMakeFs {
    async fn mkfs_on_no_wipe_volume(
        device_path: impl AsRef<Path>,
        fs_type: MakeFsType,
    ) -> Result<()> {
        let device_size = File::open(&device_path)
            .await?
            .into_std()
            .await
            .get_block_device_size()?;

        tracing::trace!(
            "The device size of {:?} is {device_size}",
            device_path.as_ref()
        );

        // Create a dummy device same size as the real one
        let dummy_device = DummyDevice::setup_on_tmpfs(device_size)
            .await
            .context("Failed to create dummy device")?;
        let dummy_device_path = dummy_device.path()?;
        let dummy_device_sector_size = File::open(&dummy_device_path)
            .await?
            .into_std()
            .await
            .get_size_of_block()?;

        // Enable the blktrace
        let tracer = BlkTrace::monitor(&dummy_device_path).await?;

        // Do some operations to the dummy device
        {
            NormalMakeFs::mkfs(&dummy_device_path, fs_type).await?;

            // TODO: refact blkid with libblkid-rs crate
            Command::new("blkid")
                .arg("-p")
                .arg(&dummy_device_path)
                .run()
                .await?;
        }

        let (events, _dropped) = tracer.shutdown().await?;
        let page_size =
            nix::unistd::sysconf(SysconfVar::PAGE_SIZE)?.context("Failed to get page size")? as u64;

        // Record all the positions touched
        let mut rw_positions: OrderSet<_> = Default::default();
        let mut r_positions: OrderSet<_> = Default::default();
        let mut w_positions: OrderSet<_> = Default::default();
        for event in &events {
            // Refer to: https://github.com/sdsc/blktrace/blob/dd093eb1c48e0d86b835758b96a9886fb7773aa4/blkparse_fmt.c#L67-L74
            /* We ignore Discard (TRIM) here */
            if !event.is_discard() && (event.is_read() || event.is_write()) {
                tracing::trace!(
                    "event action: {} sector: {}, bytes: {} bytes",
                    event.event.action,
                    event.event.sector,
                    event.event.bytes
                );

                let bytes_start = event.event.sector * dummy_device_sector_size;
                let bytes_end = bytes_start + (event.event.bytes as u64);
                // The range [bytes_start, bytes_end) is touched by the operation
                for i in (bytes_start / page_size)..((bytes_end + page_size - 1) / page_size) {
                    rw_positions.insert(i);
                    if event.is_read() {
                        r_positions.insert(i);
                    }
                    if event.is_write() {
                        w_positions.insert(i);
                    }
                }
            }
        }
        tracing::debug!(
            "Num of pages need to update to volume: {} ({} reads, {} writes), total size: {} bytes",
            rw_positions.len(),
            r_positions.len(),
            w_positions.len(),
            rw_positions.len() as u64 * page_size
        );

        // Migrate the touched pages to the real device
        async {
            let mut dummy_device_file = File::open(&dummy_device_path).await?;
            let mut real_device_file = File::options()
                .write(true)
                .read(true)
                // .custom_flags(libc::O_DIRECT)
                .open(&device_path)
                .await?;
            let mut buf = vec![0; page_size as usize];
            for i in rw_positions {
                let offset = i * page_size;
                dummy_device_file
                    .seek(std::io::SeekFrom::Start(offset))
                    .await?;
                real_device_file
                    .seek(std::io::SeekFrom::Start(offset))
                    .await?;

                dummy_device_file.read_exact(&mut buf).await?;
                real_device_file.write_all(&buf).await?;
            }
            real_device_file.flush().await?;
            Result::<_, anyhow::Error>::Ok(())
        }
        .await
        .context("Failed to migrate data from tmp device to the real device")?;

        Ok(())
    }
}

#[cfg(test)]
pub mod tests {

    use std::path::PathBuf;

    use crate::{
        async_defer,
        cli::{CloseOptions, OpenOptions},
        cmd::{close::CloseCommand, open::OpenCommand, Command as _},
        config::{
            encrypt::{EncryptConfig, KeyProviderConfig},
            volume::{ExtraConfig, VolumeConfig},
            ConfigBundle,
        },
        provider::otp::OtpConfig,
    };

    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_mkfs_with_integrity() -> Result<()> {
        let dummy_device = DummyDevice::setup_on_tmpfs(10 * 1024 * 1024 * 1024).await?;

        let volume_config = VolumeConfig {
            volume: "mkfs_with_integrity".to_owned(),
            dev: dummy_device.path().unwrap().to_str().unwrap().to_owned(),
            extra_config: ExtraConfig {
                auto_open: Some(true),
                makefs: Some(MakeFsType::Ext4),
                integrity: Some(true),
            },
            encrypt: EncryptConfig {
                key_provider: KeyProviderConfig::Otp(OtpConfig {}),
            },
        };

        crate::config::source::set_config_source(ConfigBundle {
            global: None,
            fde: None,
            volumes: vec![volume_config.clone()],
        })
        .await;

        // Close the volume if it is already opened
        CloseCommand {
            close_options: CloseOptions {
                volume: vec![volume_config.volume.clone()],
            },
        }
        .run()
        .await
        .unwrap();

        async_defer! {
            async{
                CloseCommand {
                    close_options: CloseOptions {
                        volume: vec![volume_config.volume.clone()],
                    }
                }.run().await.unwrap();
            }
        }

        OpenCommand {
            open_options: OpenOptions {
                volume: vec![volume_config.volume.clone()],
            },
        }
        .run()
        .await?;

        Command::new("blkid")
            .arg("-p")
            .arg(PathBuf::from("/dev/mapper/").join(&volume_config.volume))
            .run()
            .await?;

        Ok(())
    }
}
