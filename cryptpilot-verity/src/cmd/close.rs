use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::cmd::{Command, FUSE_FS_NAME};

pub struct CloseCommand {
    pub options: crate::cli::CloseOptions,
}

#[async_trait]
impl Command for CloseCommand {
    async fn run(&self) -> Result<()> {
        tracing::info!("Starting verity close command");
        tracing::info!("Mount point: {:?}", self.options.mount_point);

        // Validate mount point exists
        if !self.options.mount_point.exists() {
            anyhow::bail!("Mount point does not exist: {:?}", self.options.mount_point);
        }
        if !self.options.mount_point.is_dir() {
            anyhow::bail!(
                "Mount point is not a directory: {:?}",
                self.options.mount_point
            );
        }

        // Check if mount point appears to be mounted
        let is_mounted = super::is_mounted(&self.options.mount_point).await?;
        if !is_mounted {
            bail!("{FUSE_FS_NAME} seems not to be mounted at {:?}, use `umount` to force unmount if you want to", self.options.mount_point);
        }

        // Unmount using fusermount
        self.unmount(&self.options.mount_point).await?;

        tracing::info!("Successfully unmounted verity-fuse filesystem");

        Ok(())
    }
}

impl CloseCommand {
    async fn unmount(&self, mount_point: &std::path::Path) -> Result<()> {
        use std::ffi::CString;

        tracing::info!("Unmounting filesystem at: {:?}", mount_point);

        let path_str = mount_point
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in mount point path"))?;

        let c_path = CString::new(path_str)?;

        // Use libc::umount to unmount the filesystem
        let result = unsafe { libc::umount(c_path.as_ptr()) };

        if result != 0 {
            let err = std::io::Error::last_os_error();
            anyhow::bail!("Failed to unmount filesystem: {}", err);
        }

        tracing::info!("Successfully unmounted filesystem");

        Ok(())
    }
}
