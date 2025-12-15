use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use block_devs::BlckExt;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt as _,
    process::Command,
};

use crate::{
    cmd::fde::disk::{findmnt_of_dir, grub::GrubBootFdeDisk, Disk, FdeBootType, FdeDisk},
    fs::{cmd::CheckCommandOutput as _, mount::TmpMountPoint, nbd::NbdDevice},
};

/// Load the fde related config bundle from a disk device.
pub struct OnExternalFdeDisk {
    #[allow(unused)]
    nbd_device: Option<NbdDevice>,
    disk_type: ExternalDiskType,
}

enum ExternalDiskType {
    /// A normal disk which is not protected by cryptpilot
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     / -> root partition
    NoFde {
        #[allow(unused)]
        efi_dev: PathBuf,
        efi_dev_tmp_mount: TmpMountPoint,
        root_dev: PathBuf,
        #[allow(unused)]
        root_dev_tmp_mount: TmpMountPoint,
    },
    /// A disk which is protected by cryptpilot with grub as bootloader
    /// The disk mounts:
    ///     /boot/efi -> efi partition
    ///     /boot -> boot partition
    ///     / -> root partition
    Grub {
        boot_dev: PathBuf,
        boot_dev_tmp_mount: TmpMountPoint,
        #[allow(unused)]
        efi_dev: PathBuf,
        efi_dev_tmp_mount: TmpMountPoint,
    },
}

impl OnExternalFdeDisk {
    pub async fn new_from_disk(disk: &Path) -> Result<Self> {
        if !disk.exists() {
            bail!("File not exist: {disk:?}")
        }

        let real_block_device = File::open(&disk).await?.into_std().await.is_block_device();

        let (nbd_device, disk_device) = if real_block_device {
            (None, disk.to_owned())
        } else {
            // Treat it as a disk image file
            tracing::debug!(
                "The path {disk:?} is not a block device, treat it as a disk image file."
            );
            let nbd_device = NbdDevice::connect(disk).await?;
            let disk_device = nbd_device.to_path();
            (Some(nbd_device), disk_device)
        };

        // Find the EFI partition and mount it to a tmp mount point
        let efi_dev = Self::detect_efi_part(&disk_device)
            .await
            .context("Cannot found EFI partition on the disk.")?;
        let efi_dev_tmp_mount = TmpMountPoint::mount(&efi_dev, false).await?;

        // Find the boot partition and mount it to a tmp mount point
        let disk_type = match Self::detect_boot_part(&disk_device).await {
            Ok(boot_dev) => {
                let boot_dev_tmp_mount = TmpMountPoint::mount(&boot_dev, false).await?;

                ExternalDiskType::Grub {
                    boot_dev,
                    boot_dev_tmp_mount,
                    efi_dev,
                    efi_dev_tmp_mount,
                }
            }
            Err(error) => {
                tracing::warn!(?error, "Cannot found boot partition on the disk. The disk may not be a cryptpilot encrypted disk.");
                let root_dev = Self::detect_root_part(Some(&disk_device))
                    .await
                    .context("Failed to detect root partition on the disk")?;
                let root_dev_tmp_mount = TmpMountPoint::mount(&root_dev, false).await?;

                ExternalDiskType::NoFde {
                    efi_dev,
                    efi_dev_tmp_mount,
                    root_dev,
                    root_dev_tmp_mount,
                }
            }
        };

        Ok(Self {
            nbd_device,
            disk_type,
        })
    }

    pub async fn detect_root_part(hint_device: Option<&Path>) -> Result<PathBuf> {
        if hint_device.is_none() && Command::new("mountpoint").arg("/").run().await.is_ok() {
            // 1. Execute 'findmnt -n -o SOURCE /' to return the device path where '/' is mounted
            match findmnt_of_dir(Path::new("/")).await {
                Ok(source) => {
                    // Return the device path, such as /dev/sda1
                    return Ok(source);
                }
                Err(error) => {
                    tracing::warn!(?error, "Failed to find root partition from / mount point");
                }
            }
        }

        // 2. Try GPT-style LABEL match
        let mut gpt_cmd = Command::new("blkid");
        gpt_cmd.args([
            "--match-types",
            "ext4",
            "--match-token",
            r#"LABEL="root""#,
            "--list-one",
            "--output",
            "device",
        ]);

        if let Some(hint_device) = hint_device {
            gpt_cmd.arg(hint_device);
        }

        match gpt_cmd.run().await {
            Ok(gpt_stdout) => {
                let gpt_device = String::from_utf8_lossy(&gpt_stdout).trim().to_string();

                if !gpt_device.is_empty() {
                    return Ok(PathBuf::from(gpt_device));
                }
            }
            Err(error) => tracing::debug!(
                ?error,
                "Failed to detect boot partition with PARTLABEL=boot"
            ),
        }

        bail!("No boot partition found (GPT and MBR methods both failed)");
    }

    pub async fn detect_boot_part(hint_device: &Path) -> Result<PathBuf> {
        // 1. Try GPT-style PARTLABEL match
        let mut gpt_cmd = Command::new("blkid");
        gpt_cmd.args([
            "--match-types",
            "ext4",
            "--match-token",
            r#"PARTLABEL="boot""#,
            "--list-one",
            "--output",
            "device",
        ]);

        gpt_cmd.arg(hint_device);

        match gpt_cmd.run().await {
            Ok(gpt_stdout) => {
                let gpt_device = String::from_utf8_lossy(&gpt_stdout).trim().to_string();

                if !gpt_device.is_empty() {
                    return Ok(PathBuf::from(gpt_device));
                }
            }
            Err(error) => tracing::debug!(
                ?error,
                "Failed to detect boot partition with PARTLABEL=boot"
            ),
        }

        // 2. Try MBR-style fallback: search all ext4 partitions and check contents
        let output = Command::new("lsblk")
            .args(["-lnpo", "NAME,FSTYPE"])
            .run()
            .await
            .context("lsblk failed")?;

        let content = String::from_utf8_lossy(&output);
        for line in content.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 2 {
                continue;
            }
            let dev = fields[0];
            let has_boot_kernel = async {
                // Try mounting and checking for boot content
                let already_mounted = Command::new("findmnt")
                    .args(["-n", "-o", "TARGET", dev])
                    .run()
                    .await
                    .is_ok();

                if already_mounted {
                    return Ok(None);
                }
                let tmp_mount = TmpMountPoint::mount(&dev, false).await?;

                let mut has_boot_kernel = false;

                let mut entries = fs::read_dir(tmp_mount.mount_point()).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with("vmlinuz") {
                        has_boot_kernel = true;
                        break;
                    }
                }

                Ok::<_, anyhow::Error>(Some(has_boot_kernel))
            }
            .await;

            let has_boot_kernel = match has_boot_kernel {
                Ok(Some(has_boot_kernel)) => has_boot_kernel,
                Ok(None) => {
                    continue;
                }
                Err(error) => {
                    tracing::debug!(?error, dev, "Failed to check for boot kernel on device");
                    continue;
                }
            };

            if has_boot_kernel {
                return Ok(PathBuf::from(dev));
            }
        }

        bail!("No boot partition found (GPT and MBR methods both failed)");
    }

    async fn detect_efi_part(hint_device: &Path) -> Result<PathBuf> {
        // Obtain all partitions under the device
        let lsblk_stdout = {
            let mut cmd = Command::new("lsblk");
            cmd.args(["-lnpo", "NAME"]);
            cmd.arg(hint_device);
            cmd.run().await.context("Failed to list partitions")?
        };

        let lsblk_str = String::from_utf8(lsblk_stdout)?;
        let candidate_partitions = lsblk_str
            .lines()
            .filter(|line| line.chars().last().map(|c| c.is_numeric()).unwrap_or(false))
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        for part in candidate_partitions {
            let is_efi_part = async {
                // Create a temporary mount point
                let tmp_mount = TmpMountPoint::mount(&part, false).await?;
                let mount_point = tmp_mount.mount_point();

                // Check whether the EFI directory exists
                let efi_dir = mount_point.join("EFI");
                let vmlinuz_files = mount_point.join("vmlinuz-*");

                let has_efi = fs::metadata(&efi_dir).await.is_ok();
                let has_vmlinuz = glob::glob(
                    vmlinuz_files
                        .to_str()
                        .with_context(|| format!("not a valid string: {vmlinuz_files:?}"))?,
                )?
                .next()
                .is_some();

                Ok::<_, anyhow::Error>(has_efi && !has_vmlinuz)
            }
            .await;

            match is_efi_part {
                Ok(is_efi_part) => {
                    if is_efi_part {
                        return Ok(part);
                    }
                }
                Err(error) => {
                    tracing::debug!(?error, ?part, "Failed to check efi part on device");
                }
            };
        }

        bail!("No valid EFI partition found");
    }
}

#[async_trait]
impl FdeDisk for OnExternalFdeDisk {
    fn fde_boot_type(&self) -> FdeBootType {
        match self.disk_type {
            ExternalDiskType::NoFde { .. } => FdeBootType::NoFde,
            ExternalDiskType::Grub { .. } => FdeBootType::Grub,
        }
    }
}

#[async_trait]
impl Disk for OnExternalFdeDisk {
    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool> {
        Ok(self.resolve_path_on_real_disk(path)?.exists())
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>> {
        let real_path = {
            let mut path = path.to_path_buf();
            loop {
                let real_path = self.resolve_path_on_real_disk(&path)?;

                if real_path.is_symlink() {
                    let link = real_path.read_link()?;
                    path = path.join(link);
                    continue;
                }

                break real_path;
            }
        };

        let mut file = File::open(real_path).await?;
        let mut buf = vec![];
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    fn get_boot_dir_located_dev(&self) -> &Path {
        match &self.disk_type {
            ExternalDiskType::NoFde { root_dev, .. } => root_dev,
            ExternalDiskType::Grub { boot_dev, .. } => boot_dev,
        }
    }

    fn get_efi_part_root_dir(&self) -> &Path {
        match &self.disk_type {
            ExternalDiskType::NoFde {
                efi_dev_tmp_mount, ..
            }
            | ExternalDiskType::Grub {
                efi_dev_tmp_mount, ..
            } => efi_dev_tmp_mount.mount_point(),
        }
    }
}

impl OnExternalFdeDisk {
    fn resolve_path_on_real_disk(&self, path: &Path) -> Result<PathBuf> {
        if !path.starts_with("/boot") {
            bail!("The path must be start with /boot, but got {path:?}")
        }

        let real_path = match &self.disk_type {
            ExternalDiskType::NoFde {
                efi_dev_tmp_mount,
                root_dev_tmp_mount,
                ..
            } => {
                if path.starts_with("/boot/efi") {
                    efi_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/boot/efi")?)
                } else if path.starts_with("/") {
                    root_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/")?)
                } else {
                    bail!("The path must be start with /, but got {path:?}")
                }
            }
            ExternalDiskType::Grub {
                boot_dev_tmp_mount,
                efi_dev_tmp_mount,
                ..
            } => {
                if path.starts_with("/boot/efi") {
                    efi_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/boot/efi")?)
                } else {
                    boot_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/boot")?)
                }
            }
        };

        Ok(real_path)
    }
}

#[async_trait]
impl GrubBootFdeDisk for OnExternalFdeDisk {
    async fn load_global_grub_env_file(&self) -> Result<String> {
        // Try to find the GRUB environment file
        let mut grub_env_path = Path::new("/boot/grubenv");

        // If grubenv doesn't exist, try grub/grubenv
        if !matches!(self.check_file_exist_on_disk(grub_env_path), Ok(true)) {
            grub_env_path = Path::new("/boot/grub/grubenv");
        }

        if !matches!(self.check_file_exist_on_disk(grub_env_path), Ok(true)) {
            grub_env_path = Path::new("/boot/grub2/grubenv");
        }

        // Read GRUB environment
        let grub_env_content = self
            .read_file_on_disk(grub_env_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to read GRUB environment file at {:?}",
                    grub_env_path
                )
            })?;

        Ok(String::from_utf8(grub_env_content)?)
    }
}
