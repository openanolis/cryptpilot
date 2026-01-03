use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use async_trait::async_trait;
use tokio::process::Command;

use crate::{
    cmd::fde::disk::{
        grub::{FdeDiskGrubExt, GrubBootArtifacts},
        partition_table::PartitionTableType,
        uki::{FdeDiskUkiExt, UkiBootArtifacts},
    },
    fs::cmd::CheckCommandOutput as _,
};

pub mod artifacts;
pub mod current;
pub mod external;
mod grub;
mod kernel;
mod partition_table;
mod uki;

#[derive(Debug)]
pub enum FdeBootType {
    /// A normal disk which is not protected by cryptpilot
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     / -> root partition
    NoFde,
    /// A disk which is protected by cryptpilot with grub as bootloader
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     /boot -> boot partition
    ///     / -> root partition
    Grub,
    /// A disk which is protected by cryptpilot which is booted with UKI
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     / -> root partition
    Uki,
}

pub enum BootArtifactsType {
    Grub(GrubBootArtifacts),
    Uki(UkiBootArtifacts),
}

#[async_trait]
#[allow(private_bounds)]
pub trait FdeDisk: FdeDiskGrubExt + FdeDiskUkiExt {
    fn fde_boot_type(&self) -> FdeBootType;

    async fn extract_boot_artifacts(&self) -> Result<BootArtifactsType> {
        Ok(match self.fde_boot_type() {
            FdeBootType::NoFde => {
                // The disk is not a FDE disk but we assume it is disk using grub as bootloader and extract all boot artifacts
                BootArtifactsType::Grub(self.extract_boot_artifacts_grub().await?)
            }
            FdeBootType::Grub => BootArtifactsType::Grub(self.extract_boot_artifacts_grub().await?),
            FdeBootType::Uki => BootArtifactsType::Uki(self.extract_boot_artifacts_uki().await?),
        })
    }
}

#[async_trait]
trait Disk {
    /// Detect the partition table type of the disk containing /boot
    async fn detect_disk_partition_type(&self) -> Result<PartitionTableType> {
        // Get the disk device (remove partition number)
        let disk_device = self.get_disk_root_device(self.get_boot_dir_located_dev()?)?;

        // Read the first sector of the disk to determine partition table type
        PartitionTableType::detect_partition_table_type(&disk_device).await
    }

    /// Get the disk device path from a partition device path
    fn get_disk_root_device(&self, part_dev: &Path) -> Result<PathBuf> {
        let part_dev_str = part_dev.to_string_lossy();

        // Get the disk device (remove partition number)
        if let Some(pos) = part_dev_str.rfind(|c: char| c.is_ascii_digit()) {
            // Find the last digit and remove everything from there
            let mut disk = part_dev_str[..pos].to_string();
            // Handle special case for nvme devices (e.g., /dev/nvme0n1p3 -> /dev/nvme0n1)
            if disk.ends_with('p') {
                disk.pop(); // Remove the 'p'
            }
            Ok(PathBuf::from(disk))
        } else {
            Ok(part_dev.to_path_buf())
        }
    }

    /// Get the path of block device where /boot is located
    fn get_boot_dir_located_dev(&self) -> Result<&Path>;

    async fn read_file_on_disk_to_string(&self, path: &Path) -> Result<String> {
        self.read_file_on_disk(path)
            .await
            .and_then(|v| anyhow::Ok(String::from_utf8(v)?))
    }

    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool>;

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>>;

    fn get_efi_part_root_dir(&self) -> &Path;
}

pub async fn findmnt_of_dir(dir: &Path) -> Result<PathBuf> {
    let mut cmd = Command::new("findmnt");
    cmd.args(["-n", "-o", "SOURCE"]);
    cmd.arg(dir);
    let stdout = cmd.run().await?;
    let dev = PathBuf::from(String::from_utf8(stdout)?.trim().to_string());
    if !dev.exists() {
        bail!("mount source of {dir:?} is {dev:?} but not exists");
    }
    Ok(dev)
}
