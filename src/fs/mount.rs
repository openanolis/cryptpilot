use std::{
    future::Future,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use tokio::process::Command;

use crate::async_defer;

use super::cmd::CheckCommandOutput as _;

pub struct TmpMountPoint {}

impl TmpMountPoint {
    pub async fn with_new_mount<F, Fut, R>(dev: impl AsRef<Path>, func: F) -> Result<R>
    where
        F: FnOnce(PathBuf) -> Fut,
        Fut: Future<Output = R>,
    {
        let mount_dir = tempfile::Builder::new()
            .prefix("cryptpilot-mount-")
            .tempdir()?;

        let mount_point = mount_dir.path();

        let dev = dev.as_ref();
        Command::new("mount")
            .arg(dev)
            .arg(mount_point)
            .run()
            .await
            .with_context(|| format!("Failed to mount {dev:?}"))?;

        async_defer! {
            async{
                Command::new("umount")
                    .arg(mount_point)
                    .run()
                    .await
                    .with_context(|| format!("Failed to umount {dev:?}"))?;
                Ok::<_, anyhow::Error>(())
            }
        }

        let ret = func(mount_point.to_path_buf()).await;

        Ok(ret)
    }
}
