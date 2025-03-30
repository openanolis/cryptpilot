use std::{
    io::{Seek, Write},
    path::PathBuf,
};

use anyhow::{Context as _, Result};
use loopdev::{LoopControl, LoopDevice};
use tempfile::NamedTempFile;

pub struct DummyDevice {
    #[allow(unused)]
    sparse_file: NamedTempFile,
    ld: LoopDevice,
}

impl DummyDevice {
    pub async fn setup(device_size: u64) -> Result<Self> {
        let mut sparse_file = tempfile::Builder::new()
            .prefix("cryptpilot-")
            .suffix(".img")
            .tempfile()
            .context("Failed to create sparse file")?;

        sparse_file.seek(std::io::SeekFrom::Start(device_size - 1))?;
        sparse_file.write_all(&[0])?;

        let lc = LoopControl::open()?;
        let ld = lc.next_free()?;
        ld.attach_file(sparse_file.path())?;

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

#[cfg(test)]
pub mod tests {

    use std::os::linux::fs::MetadataExt;

    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_sparse_file() -> Result<()> {
        let size = 1024 * 1024 * 1024;
        let dummy_device = DummyDevice::setup(size).await?;

        let metadata = tokio::fs::metadata(dummy_device.sparse_file.path()).await?;

        assert!(metadata.st_blocks() * metadata.st_blksize() < size);

        Ok(())
    }
}
