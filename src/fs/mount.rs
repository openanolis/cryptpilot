use std::path::{Path, PathBuf};

use anyhow::anyhow;
use anyhow::{Context as _, Result};
use tempfile::TempDir;
use tokio::process::Command;

use crate::async_defer;

use super::cmd::CheckCommandOutput as _;

pub struct TmpMountPoint {
    mount_dir: TempDir,
    dev: PathBuf,
}

impl TmpMountPoint {
    pub async fn mount(dev: impl AsRef<Path>) -> Result<Self> {
        let dev = dev.as_ref();

        let mount_dir = tempfile::Builder::new()
            .prefix("cryptpilot-mount-")
            .tempdir()?;
        let mount_point = mount_dir.path();

        Command::new("mount")
            .arg(dev)
            .arg(mount_point)
            .run()
            .await
            .with_context(|| format!("Failed to mount {dev:?}"))?;

        Ok(Self {
            mount_dir,
            dev: dev.to_path_buf(),
        })
    }

    pub fn mount_point(&self) -> &Path {
        self.mount_dir.path()
    }
}

impl Drop for TmpMountPoint {
    fn drop(&mut self) {
        async_defer! {
            async{
                let mount_point = self.mount_dir.path();
                Command::new("umount")
                    .arg(mount_point)
                    .run()
                    .await
                    .with_context(|| format!("Failed to umount device {:?} from {:?}", self.dev, mount_point))?;
                Ok::<_, anyhow::Error>(())
            }
        }
    }
}
