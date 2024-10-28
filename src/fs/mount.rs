use std::{future::Future, path::PathBuf};

use anyhow::{Context as _, Result};
use tempfile::TempDir;
use tokio::process::Command;

use super::cmd::CheckCommandOutput as _;

pub struct TmpMountPoint {}

impl TmpMountPoint {
    pub async fn with_new_mount<F, Fut, R>(dev: &str, func: F) -> Result<R>
    where
        F: FnOnce(PathBuf) -> Fut,
        Fut: Future<Output = R>,
    {
        let temp_dir = TempDir::new()?;
        let mount_point = temp_dir.path();

        Command::new("mount")
            .arg(dev)
            .arg(mount_point)
            .run_check_output()
            .await
            .with_context(|| format!("Failed to mount {dev}"))?;

        let ret = func(mount_point.to_path_buf()).await;

        Command::new("umount")
            .arg(mount_point)
            .run_check_output()
            .await
            .with_context(|| format!("Failed to umount {dev}"))?;

        Ok(ret)
    }
}
