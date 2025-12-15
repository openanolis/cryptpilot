use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use tokio::{fs::File, io::AsyncReadExt as _, process::Command};

use crate::{
    cmd::fde::disk::{findmnt_of_dir, grub::GrubBootFdeDisk, Disk, FdeBootType, FdeDisk},
    fs::cmd::CheckCommandOutput as _,
};

/// Load the fde related config bundle from current system. This should be used
/// only when the system is booted into the system manager (systemd) stage, and
/// should not be used in initrd stage.
#[non_exhaustive]
pub struct OnCurrentSystemFdeDisk {
    disk_type: ExternalDiskType,
}

enum ExternalDiskType {
    /// A normal disk which is not protected by cryptpilot
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     / -> root partition
    NoFde { root_dev: PathBuf },
    /// A disk which is protected by cryptpilot with grub as bootloader
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     /boot -> boot partition
    ///     / -> root partition
    Grub { boot_dev: PathBuf },
}

impl OnCurrentSystemFdeDisk {
    pub async fn new() -> Result<Self> {
        // Find the block device that contains /boot mount point
        let boot_dev = findmnt_of_dir(Path::new("/boot"))
            .await
            .context("Failed to determine /boot mount source");

        Ok(match boot_dev {
            Ok(boot_dev) => Self {
                disk_type: ExternalDiskType::Grub { boot_dev },
            },
            Err(error) => {
                tracing::warn!(?error, "Cannot found boot partition on the disk. The disk may not be a cryptpilot encrypted disk.");

                let root_dev = findmnt_of_dir(Path::new("/"))
                    .await
                    .context("Failed to determine / mount source")?;

                Self {
                    disk_type: ExternalDiskType::NoFde { root_dev },
                }
            }
        })
    }
}

#[async_trait]
impl FdeDisk for OnCurrentSystemFdeDisk {
    fn fde_boot_type(&self) -> FdeBootType {
        match self.disk_type {
            ExternalDiskType::NoFde { .. } => FdeBootType::NoFde,
            ExternalDiskType::Grub { .. } => FdeBootType::Grub,
        }
    }
}

#[async_trait]
impl Disk for OnCurrentSystemFdeDisk {
    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool> {
        Ok(path.exists())
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>> {
        let mut file = File::open(path).await?;
        let mut buf = vec![];
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    fn get_boot_dir_located_dev(&self) -> &Path {
        match &self.disk_type {
            ExternalDiskType::NoFde { root_dev } => root_dev,
            ExternalDiskType::Grub { boot_dev } => boot_dev,
        }
    }

    fn get_efi_part_root_dir(&self) -> &Path {
        Path::new("/boot/efi")
    }
}

#[async_trait]
impl GrubBootFdeDisk for OnCurrentSystemFdeDisk {
    async fn load_global_grub_env_file(&self) -> Result<String> {
        // Get the saved entry from GRUB environment
        let stdout = Command::new("grub2-editenv").arg("list").run().await?;

        let grub_env = String::from_utf8(stdout)?;

        Ok(grub_env)
    }
}
