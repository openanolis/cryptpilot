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
    fs::{block::dummy::DummyDevice, cmd::CheckCommandOutput as _},
    types::{IntegrityType, MakeFsType},
};

use super::block::blktrace::BlkTrace;

#[async_trait]
pub trait MakeFs {
    async fn force_mkfs(
        device_path: impl AsRef<Path> + Send + Sync,
        fs_type: MakeFsType,
    ) -> Result<()>;
}

pub struct NormalMakeFs;

#[async_trait]
impl MakeFs for NormalMakeFs {
    async fn force_mkfs(
        device_path: impl AsRef<Path> + Send + Sync,
        fs_type: MakeFsType,
    ) -> Result<()> {
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
    async fn force_mkfs(
        device_path: impl AsRef<Path> + Send + Sync,
        fs_type: MakeFsType,
    ) -> Result<()> {
        let (device_size, block_size) = {
            let file = File::open(&device_path).await?.into_std().await;
            (file.get_block_device_size()?, file.get_size_of_block()?)
        };

        tracing::info!("Setup dummy device with {device_size} bytes size for recording");

        // Create a dummy device same size as the real one
        let dummy_device = DummyDevice::setup_on_tmpfs_with_block_size(device_size, block_size)
            .await
            .context("Failed to create dummy device")?;
        let dummy_device_path = dummy_device.path()?;

        // Enable the blktrace
        tracing::info!("Starting to record device operations");
        let tracer = BlkTrace::monitor(&dummy_device_path).await?;
        tracing::trace!(
            device = ?dummy_device_path,  "start blktrace on device"
        );

        // Do some operations to the dummy device
        {
            NormalMakeFs::force_mkfs(&dummy_device_path, fs_type).await?;

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

                // Linux always considers sectors to be 512 bytes long independently
                // https://github.com/torvalds/linux/blob/7839932417dd53bb09eb5a585a7a92781dfd7cb2/include/linux/types.h#L132
                let bytes_start = event.event.sector * 512;
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
        tracing::info!(
            "Recording finished, num of pages need to update to volume: {} ({} reads, {} writes), total size: {} bytes",
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

        tracing::info!("Replaying data to the real device finished");
        Ok(())
    }
}

pub async fn force_mkfs(
    volume_path: &Path,
    makefs: &MakeFsType,
    integrity: IntegrityType,
) -> Result<()> {
    let volume_path = volume_path.to_owned();
    let makefs = makefs.to_owned();

    tracing::info!(
        "Initializing {} fs on volume {:?}, with volume integrity type {:?}",
        makefs,
        volume_path,
        integrity
    );
    match integrity {
        IntegrityType::None => NormalMakeFs::force_mkfs(&volume_path, makefs).await,
        IntegrityType::Journal | IntegrityType::NoJournal => {
            IntegrityNoWipeMakeFs::force_mkfs(&volume_path, makefs).await
        }
    }
    .with_context(|| format!("Failed to initialize {makefs} fs on volume {volume_path:?}"))?;
    Ok(())
}

pub async fn is_empty_disk(device_path: &Path) -> Result<bool> {
    Command::new("blkid")
        .arg("-p")
        .arg(device_path)
        .env("LC_ALL", "C")
        .run_with_status_checker(|code, stdout, stderr| {
            match code {
                0 => {
                    // Found signatures, check if they are ext4, xfs, vfat or swap
                    let output = String::from_utf8_lossy(&stdout);
                    let has_fs = output.contains("TYPE=\"ext4\"")
                        || output.contains("TYPE=\"xfs\"")
                        || output.contains("TYPE=\"vfat\"")
                        || output.contains("TYPE=\"swap\"");
                    Ok(!has_fs) // Return true if NOT one of these filesystems (empty or other fs)
                }
                2 => Ok(true), // No signatures found
                _ => {
                    let stdout = String::from_utf8_lossy(&stdout);
                    let stderr = String::from_utf8_lossy(&stderr);
                    if stdout.contains("Input/output error")
                        || stderr.contains("Input/output error")
                    {
                        // If we can't even read the disk, treat it as "uninitialized"
                        Ok(true)
                    } else {
                        bail!(
                            "blkid failed with exit code {}: stdout={}, stderr={}",
                            code,
                            stdout,
                            stderr
                        )
                    }
                }
            }
        })
        .await
        .context("Failed to detect filesystem signatures using blkid")
}
