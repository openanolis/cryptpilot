use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use block_devs::BlckExt as _;
use nix::unistd::SysconfVar;
use ordermap::OrderSet;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    process::Command,
};

use crate::{
    config::volume::MakeFsType,
    fs::{
        block::{
            blktrace::{blktrace_cat_BLK_TC_DISCARD, BLK_TC_SHIFT},
            dummy::DummyDevice,
        },
        cmd::CheckCommandOutput as _,
    },
};

use super::{
    block::blktrace::{blktrace_cat_BLK_TC_READ, blktrace_cat_BLK_TC_WRITE, BlkTrace},
    shell::Shell,
};

#[async_trait]
pub trait MakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()>;
}

pub struct NormalMakeFs;

#[async_trait]
impl MakeFs for NormalMakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()> {
        // There is no need to check volume here since systemd-makefs will check it.
        Command::new("/usr/lib/systemd/systemd-makefs")
            .arg(fs_type.to_systemd_makefs_fstype())
            .arg(device_path.as_ref())
            .run_check_output()
            .await?;
        Ok(())
    }
}

impl MakeFsType {
    fn to_systemd_makefs_fstype(&self) -> &'static str {
        match self {
            MakeFsType::Swap => "swap",
            MakeFsType::Ext4 => "ext4",
            MakeFsType::Xfs => "xfs",
            MakeFsType::Vfat => "vfat",
        }
    }
}

pub struct IntegrityNoWipeMakeFs;

#[async_trait]
impl MakeFs for IntegrityNoWipeMakeFs {
    async fn mkfs(device_path: impl AsRef<Path> + Send + Sync, fs_type: MakeFsType) -> Result<()> {
        let is_empty_disk = {
            let device_path: PathBuf = device_path.as_ref().to_owned();
            tokio::task::spawn_blocking(move || -> Result<_> {
                Shell(format!(
                    r#"
                export LC_ALL=C
                set +o errexit
                res=`file -E --brief --dereference --special-files {:?}`
                status=$?
                set -o errexit

                if [[ $res == *"Input/output error"* ]] || [[ $res == "data" ]] ; then
                    # A uninitialized (empty) volume
                    exit 2
                elif [[ $status -ne 0 ]] ; then
                    # Error happens
                    echo $res >&2
                    exit 1
                else
                    # Maybe some thing on the volume, so we should not touch it.
                    exit 3
                fi
            "#,
                    device_path,
                ))
                .run_with_status_checker(|code, _, _| match code {
                    2 => Ok(true),
                    3 => Ok(false),
                    _ => {
                        bail!("Bad exit code")
                    }
                })
                .with_context(|| format!("Failed to detecting filesystem type",))
            })
            .await
            .context("background task failed")??
        };

        if is_empty_disk {
            Self::mkfs_on_no_wipe_volume(device_path, fs_type).await?
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
                .run_check_output()
                .await?;
        }

        let (events, _dropped) = tracer.shutdown().await?;
        let page_size =
            nix::unistd::sysconf(SysconfVar::PAGE_SIZE)?.context("Failed to get page size")? as u64;

        // Record all the positions touched
        let mut rw_positions: OrderSet<_> = Default::default();
        for event in &events {
            // Refer to: https://github.com/sdsc/blktrace/blob/dd093eb1c48e0d86b835758b96a9886fb7773aa4/blkparse_fmt.c#L67-L74
            if (((event.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_DISCARD)
                != blktrace_cat_BLK_TC_DISCARD) /* We ignore Discard (TRIM) here */
                && ((((event.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_READ)
                    == blktrace_cat_BLK_TC_READ)
                    || (((event.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_WRITE)
                        == blktrace_cat_BLK_TC_WRITE))
            {
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
                }
            }
        }
        tracing::debug!(
            "Num of pages need to update to volume: {}, total size: {} bytes",
            rw_positions.len(),
            rw_positions.len() as u64 * page_size
        );

        // Migrate the touched pages to the real device
        async {
            let mut dummy_device_file = File::open(&dummy_device_path).await?;
            let mut real_device_file = OpenOptions::new()
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
                real_device_file.write(&buf).await?;
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

    use crate::{
        async_defer,
        cli::CloseOptions,
        cmd::{close::CloseCommand, Command as _},
        config::{
            encrypt::{EncryptConfig, KeyProviderConfig},
            volume::{ExtraConfig, VolumeConfig},
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

        // Close the volume if it is already opened
        CloseCommand {
            close_options: CloseOptions {
                volume: volume_config.volume.clone(),
            },
        }
        .run()
        .await
        .unwrap();

        async_defer! {
            async{
                CloseCommand{ close_options: CloseOptions{volume: volume_config.volume.to_owned()}}.run().await.unwrap();
            }
        }

        crate::cmd::open::open_for_specific_volume(&volume_config).await?;

        Command::new("blkid")
            .arg("-p")
            .arg(PathBuf::from("/dev/mapper/").join(&volume_config.volume))
            .run_check_output()
            .await?;

        Ok(())
    }
}
