use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use tokio::{fs::File, io::AsyncReadExt as _, process::Command};

use crate::disk::{
    findmnt_of_dir, grub::FdeDiskGrubExt, uki::UKI_FILE_PATH, Disk, FdeBootType, FdeDisk,
    FdeDiskUkiExt,
};
use cryptpilot::fs::cmd::CheckCommandOutput as _;

/// Load the fde related config bundle from current system. This should be used
/// only when the system is booted into the system manager (systemd) stage, and
/// should not be used in initrd stage.
#[non_exhaustive]
pub struct OnCurrentSystemFdeDisk {
    disk_type: ExternalDiskType,
}

enum ExternalDiskType {
    NoFde { root_dev: PathBuf },
    Grub { boot_dev: PathBuf },
    Uki,
}

impl OnCurrentSystemFdeDisk {
    pub async fn new() -> Result<Self> {
        // Find the BOOTX64.EFI in the EFI partition
        {
            let file = Path::new(UKI_FILE_PATH);
            if file.exists() {
                tracing::debug!("Found BOOTX64.EFI in the EFI partition, checking...");
                match tokio::fs::read(&file)
                    .await
                    .with_context(|| format!("Failed to read {file:?}"))
                    .and_then(|bytes| crate::disk::uki::assume_uki_image(&bytes))
                {
                    Ok(()) => {
                        return Ok(Self {
                            disk_type: ExternalDiskType::Uki,
                        });
                    }
                    Err(error) => {
                        tracing::warn!(?error, ?file, "This disk is not a UKI booted disk since BOOTX64.EFI is not a valid UKI image.");
                    }
                }
            }
        }

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
            ExternalDiskType::Uki { .. } => FdeBootType::Uki,
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

    fn get_boot_dir_located_dev(&self) -> Result<&Path> {
        match &self.disk_type {
            ExternalDiskType::NoFde { root_dev } => Ok(root_dev),
            ExternalDiskType::Grub { boot_dev } => Ok(boot_dev),
            ExternalDiskType::Uki { .. } => bail!("Not supported for UKI"),
        }
    }

    fn get_efi_part_root_dir(&self) -> &Path {
        Path::new("/boot/efi")
    }
}

#[async_trait]
impl FdeDiskGrubExt for OnCurrentSystemFdeDisk {
    async fn load_global_grub_env_file(&self) -> Result<String> {
        // Get the saved entry from GRUB environment
        let stdout = Command::new("grub2-editenv").arg("list").run().await?;

        let grub_env = String::from_utf8(stdout)?;

        Ok(grub_env)
    }
}

#[async_trait]
impl FdeDiskUkiExt for OnCurrentSystemFdeDisk {}
