use std::{
    io::{Seek, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    time::Duration,
};

use again::RetryPolicy;
use anyhow::{Context as _, Result};
use loopdev::{LoopControl, LoopDevice};
use tempfile::NamedTempFile;

const BLOCK_SIZE_DEFAULT: u64 = 512;

pub struct DummyDevice {
    #[allow(unused)]
    sparse_file: NamedTempFile,
    ld: LoopDevice,
}

impl DummyDevice {
    #[allow(unused)]
    pub async fn setup_on_tmpfs_with_block_size(device_size: u64, block_size: u64) -> Result<Self> {
        Self::setup(device_size, block_size, true).await
    }

    #[allow(unused)]
    pub async fn setup_on_tmpfs(device_size: u64) -> Result<Self> {
        Self::setup(device_size, BLOCK_SIZE_DEFAULT, true).await
    }

    #[allow(unused)]
    pub async fn setup_on_cache_dir(device_size: u64) -> Result<Self> {
        Self::setup(device_size, BLOCK_SIZE_DEFAULT, false).await
    }

    async fn setup(device_size: u64, block_size: u64, on_tmpfs: bool) -> Result<Self> {
        let mut sparse_file = tempfile::Builder::new()
            .prefix("cryptpilot-")
            .suffix(".img")
            .tempfile_in(if on_tmpfs {
                std::env::temp_dir()
            } else {
                match dirs::cache_dir() {
                    Some(dir) => {
                        let dir = dir.join("cryptpilot");
                        match tokio::fs::create_dir_all(&dir).await {
                            Ok(()) => dir,
                            Err(_) => std::env::temp_dir(),
                        }
                    }
                    None => std::env::temp_dir(),
                }
            })
            .context("Failed to create sparse file")?;

        sparse_file.seek(std::io::SeekFrom::Start(device_size - 1))?;
        sparse_file.write_all(&[0])?;

        let lc = LoopControl::open()
            .context("Failed to open loop control, maybe forgot to run 'sudo modprobe loop'?")?;
        // Retry to avoid conflicts and waiting for avaliable loop device
        let ld = RetryPolicy::exponential(Duration::from_millis(1))
            .with_max_retries(200)
            .with_max_delay(Duration::from_millis(1000))
            .retry(|| async {
                let ld = lc.next_free()?;
                ld.with().attach(sparse_file.path())?;
                loop_device_set_block_size(&ld, block_size)?;
                Ok::<_, anyhow::Error>(ld)
            })
            .await?;

        Ok(DummyDevice { sparse_file, ld })
    }

    pub fn path(&self) -> Result<PathBuf> {
        self.ld.path().context("Unknown loop device path")
    }
}

impl Drop for DummyDevice {
    fn drop(&mut self) {
        if let Err(e) = self
            .ld
            .detach()
            .with_context(|| format!("Failed to detach loop device {:?}", self.ld.path()))
        {
            tracing::error!("{e:#}")
        };
    }
}

const LOOP_SET_BLOCK_SIZE: u64 = 0x4C09;

fn loop_device_set_block_size(ld: &LoopDevice, block_size: u64) -> Result<()> {
    let _ = unsafe {
        nix::errno::Errno::result(libc::ioctl(ld.as_raw_fd(), LOOP_SET_BLOCK_SIZE, block_size))
    }
    .context("Failed to LOOP_SET_BLOCK_SIZE")?;
    Ok(())
}

#[cfg(test)]
pub mod tests {

    use std::os::linux::fs::MetadataExt;

    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_sparse_file() -> Result<()> {
        let size = 1024 * 1024 * 1024;
        let dummy_device = DummyDevice::setup_on_tmpfs(size).await?;

        let metadata = tokio::fs::metadata(dummy_device.sparse_file.path()).await?;

        assert!(metadata.st_blocks() * metadata.st_blksize() < size);

        Ok(())
    }
}
