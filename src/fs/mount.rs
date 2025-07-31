use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use tempfile::TempDir;
use tokio::process::Command;
use anyhow::anyhow;

use crate::async_defer;

use super::cmd::CheckCommandOutput as _;

pub struct TmpMountPoint {
    mount_dir: TempDir,
    dev: PathBuf,
}

impl TmpMountPoint {
    pub async fn mount(dev: impl AsRef<Path>) -> Result<Self> {

        let dev = dev.as_ref();

        // Check whether the equipment has been mounted
        let dev_str = dev.as_os_str()
            .to_str()
            .ok_or_else(|| anyhow!("Non-UTF8 device path"))?;

        let mut command = Command::new("findmnt");
        command.args(["-n", "-o", "TARGET", dev_str]);

        let output = match command.run().await {
            Ok(stdout) => stdout,
            Err(e) => {
                tracing::warn!("findmnt failed: {}", e);
                Vec::new()
            }
        };

        let existing_mount = String::from_utf8_lossy(&output).trim().to_string();

        if existing_mount.is_empty() {
            tracing::info!("Device {dev:?} is not mounted or findmnt failed.");
        } else {
            tracing::info!("Device {dev:?} already mounted at {existing_mount}");
        }

        if existing_mount == "/boot" {
            let temp_dir = tempfile::Builder::new()
                .prefix("cryptpilot-mount-")
                .tempdir()?;

            let temp_dir_str = temp_dir.path()
                .to_str()
                .ok_or_else(|| anyhow!("Invalid UTF-8 in temp dir path"))?;

            let mut mount_cmd = Command::new("mount");
            mount_cmd.args(["--bind", "/boot", temp_dir_str]);

            mount_cmd.run().await.map_err(|e| {
                anyhow::anyhow!("Failed to bind-mount /boot to temp dir: {}", e)
            })?;

            return Ok(Self {
                mount_dir: temp_dir,
                dev: dev.to_path_buf(),
            });
        }

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
