use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context as _, Result};
use async_trait::async_trait;
use authenticode::PeTrait;
use block_devs::BlckExt;
use futures_lite::stream::StreamExt;
use object::read::pe::{PeFile32, PeFile64};
use sha2::Digest;
use tempfile::tempdir;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt as _,
    process::Command,
};

use crate::{
    cmd::boot_service::metadata::Metadata,
    config::source::{cloud_init::FdeConfigBundle, fs::FileSystemConfigSource, ConfigSource},
    fs::{cmd::CheckCommandOutput as _, mount::TmpMountPoint, nbd::NbdDevice},
};

/// Partition table type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartitionTableType {
    Mbr,
    Gpt,
}

/// Structure to hold kernel and initrd information
#[derive(Debug, Clone)]
pub struct BootMeasurement {
    pub kernel_cmdline: String,
    pub kernel_cmdline_sha384: String,
    pub kernel_sha384: String,
    pub initrd_sha384: String,
    pub grub_authenticode_sha384: String,
    pub shim_authenticode_sha384: String,
}

#[async_trait]
pub trait FdeDisk: Send + Sync {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle>;

    async fn load_metadata(&self) -> Result<Metadata>;

    async fn get_boot_measurement(&self) -> Result<BootMeasurement> {
        // Get the saved entry from GRUB environment
        let grub_env = self.get_grub_env().await?;

        // Parse GRUB environment variables
        let mut grub_vars = std::collections::HashMap::new();
        for line in grub_env.lines() {
            if let Some(eq_pos) = line.find('=') {
                let key = &line[..eq_pos];
                let value = &line[eq_pos + 1..];
                grub_vars.insert(key.to_string(), value.to_string());
            }
        }

        // Set default empty values for tuned_* variables if not present
        if !grub_vars.contains_key("tuned_params") {
            grub_vars.insert("tuned_params".to_string(), String::new());
        }
        if !grub_vars.contains_key("tuned_initrd") {
            grub_vars.insert("tuned_initrd".to_string(), String::new());
        }

        // Get the GRUB config content to find kernelopts if not in grubenv
        if !grub_vars.contains_key("kernelopts") {
            let grub_cfg_content = self.get_grub_config().await?;
            // Look for kernelopts definition in the GRUB config
            for line in grub_cfg_content.lines() {
                if line.contains("set kernelopts=") {
                    if let Some(opts) = line.strip_prefix("set kernelopts=") {
                        let opts_value = opts.trim().trim_matches('"').to_string();
                        grub_vars.insert("kernelopts".to_string(), opts_value);
                        break;
                    }
                }
            }

            // If still not found, look for fallback kernelopts definition
            let lines: Vec<&str> = grub_cfg_content.lines().collect();
            for i in 0..lines.len() {
                if lines[i].contains("if [ -z \\\"${kernelopts}\\\" ]; then") {
                    // Look for the next line with set kernelopts
                    if i + 1 < lines.len() {
                        let next_line = lines[i + 1];
                        if next_line.contains("set kernelopts=") {
                            if let Some(opts) = next_line.strip_prefix("  set kernelopts=") {
                                let opts_value = opts.trim().trim_matches('"').to_string();
                                grub_vars.insert("kernelopts".to_string(), opts_value);
                                break;
                            }
                        }
                    }
                }
            }
        }

        self.process_boot_measurement(&grub_vars).await
    }

    /// Get GRUB environment variables
    async fn get_grub_env(&self) -> Result<String>;

    /// Get GRUB configuration file content
    async fn get_grub_config(&self) -> Result<String> {
        match self
            .read_file_on_disk_to_string(Path::new("/boot/grub2/grub.cfg"))
            .await
        {
            Ok(content) => Ok(content),
            Err(e) => {
                tracing::warn!(
                    error=?e,
                    "Failed to read GRUB config file at /boot/grub2/grub.cfg"
                );

                // Try alternative path
                match self
                    .read_file_on_disk_to_string(Path::new("/boot/efi/EFI/alinux/grub.cfg"))
                    .await
                {
                    Ok(content) => Ok(content),
                    Err(e) => {
                        tracing::warn!(
                            error=?e,
                            "Failed to read GRUB config file at /boot/efi/EFI/alinux/grub.cfg"
                        );
                        Err(e).context("Failed to read GRUB config file")
                    }
                }
            }
        }
    }

    async fn process_boot_measurement(
        &self,
        grub_vars: &std::collections::HashMap<String, String>,
    ) -> Result<BootMeasurement> {
        let saved_entry = grub_vars
            .get("saved_entry")
            .ok_or_else(|| anyhow::anyhow!("saved_entry not found in GRUB environment"))?;

        // Read the corresponding loader entry file
        let entry_content = {
            let entry_path = format!("/boot/loader/entries/{}.conf", saved_entry);
            self.read_file_on_disk_to_string(Path::new(&entry_path))
                .await
                .with_context(|| format!("Failed to read loader entry file {}", entry_path))?
        };

        // Parse the kernel path, cmdline, and initrd path
        let mut kernel_path = String::new();
        let mut cmdline = String::new();
        let mut initrd_path = String::new();

        for line in entry_content.lines() {
            if line.starts_with("linux ") {
                kernel_path = line.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("options ") {
                cmdline = line.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string();
            } else if line.starts_with("initrd ") {
                initrd_path = line.splitn(2, ' ').nth(1).unwrap_or("").trim().to_string();
            }
        }

        // Substitute GRUB variables in cmdline and initrd path
        for (key, value) in grub_vars {
            let var_pattern = format!("${}", key);
            cmdline = cmdline.replace(&var_pattern, value);
            initrd_path = initrd_path.replace(&var_pattern, value);
        }
        cmdline = cmdline.replace("  ", " ");
        cmdline = cmdline.trim().to_string();

        // Clean up the paths by removing any remaining variables or extra text
        if let Some(space_pos) = kernel_path.find(' ') {
            kernel_path = kernel_path[..space_pos].to_string();
        }

        if let Some(space_pos) = initrd_path.find(' ') {
            initrd_path = initrd_path[..space_pos].to_string();
        }

        // Make kernel path absolute if needed
        if !kernel_path.is_empty() {
            if kernel_path.starts_with("/") {
                // Already absolute
            } else {
                kernel_path = format!("/boot/{}", kernel_path);
            }
        }

        // Make initrd path absolute if needed
        if !initrd_path.is_empty() {
            if initrd_path.starts_with("/") {
                // Already absolute
            } else {
                initrd_path = format!("/boot/{}", initrd_path);
            }
        }

        let kernel_path = Path::new(&kernel_path);

        // Calculate SHA384 hashes
        let kernel_sha384 = {
            let content = self
                .read_file_on_disk(&kernel_path)
                .await
                .with_context(|| format!("Failed to read kernel file at {:?}", kernel_path))?;
            let mut hasher = sha2::Sha384::new();
            hasher.update(&content);
            format!("{:x}", hasher.finalize())
        };

        let initrd_sha384 = {
            let content = self
                .read_file_on_disk(Path::new(&initrd_path))
                .await
                .with_context(|| format!("Failed to read initrd file at {}", initrd_path))?;
            let mut hasher = sha2::Sha384::new();
            hasher.update(&content);
            format!("{:x}", hasher.finalize())
        };

        // Construct full kernel command line with inferred device identifier prefix
        let full_kernel_cmdline = {
            // Infer device identifier from current boot partition device path
            let device_identifier = {
                // Detect partition table type
                let partition_type = self.detect_disk_type().await?;

                // Extract partition number from device path
                // For example, /dev/sda3 -> (hd0,gpt3) or (hd0,msdos3), /dev/nvme0n1p3 -> (hd0,gpt3) or (hd0,msdos3)
                let boot_dev = self.get_boot_dev();
                if let Some(partition_num) = boot_dev
                    .to_string_lossy()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .parse::<u32>()
                    .ok()
                {
                    match partition_type {
                        PartitionTableType::Gpt => format!("(hd0,gpt{})", partition_num),
                        PartitionTableType::Mbr => format!("(hd0,msdos{})", partition_num),
                    }
                } else {
                    bail!(
                        "Unable to extract partition number from boot device path: {:?}",
                        boot_dev
                    );
                }
            };

            // Combine device identifier with kernel path and command line arguments
            format!(
                "{}{} {}",
                device_identifier,
                kernel_path
                    .strip_prefix("/")
                    .unwrap_or(&kernel_path)
                    .to_string_lossy(),
                cmdline
            )
        };

        let kernel_cmdline_sha384 = {
            let mut hasher = sha2::Sha384::new();
            hasher.update(&full_kernel_cmdline);
            format!("{:x}", hasher.finalize())
        };

        // Try to read GRUB and SHIM for measurement
        let (grub_authenticode_sha384, shim_authenticode_sha384) = {
            let (grub_data, shim_data) = self
                .read_grub_and_shim()
                .await
                .context("Failed to read GRUB and SHIM binaries")?;

            // Calculate SHA384 hashes for GRUB and SHIM
            let grub_hash = {
                let mut hasher = sha2::Sha384::new();
                let pe = parse_pe(&grub_data)?;
                authenticode::authenticode_digest(&*pe, &mut hasher)?;
                format!("{:x}", hasher.finalize())
            };

            let shim_hash = {
                let mut hasher = sha2::Sha384::new();
                let pe = parse_pe(&shim_data)?;
                authenticode::authenticode_digest(&*pe, &mut hasher)?;
                format!("{:x}", hasher.finalize())
            };

            (grub_hash, shim_hash)
        };

        Ok(BootMeasurement {
            kernel_cmdline: full_kernel_cmdline,
            kernel_cmdline_sha384,
            kernel_sha384,
            initrd_sha384,
            grub_authenticode_sha384,
            shim_authenticode_sha384,
        })
    }

    /// Detect the partition table type of the disk containing /boot
    async fn detect_disk_type(&self) -> Result<PartitionTableType> {
        // Get the disk device (remove partition number)
        let disk_device = self.get_disk_device(self.get_boot_dev())?;

        // Read the first sector of the disk to determine partition table type
        self.detect_partition_table_type(&disk_device).await
    }

    /// Get the disk device path from a partition device path
    fn get_disk_device(&self, boot_dev: &Path) -> Result<PathBuf> {
        let boot_dev_str = boot_dev.to_string_lossy();

        // Get the disk device (remove partition number)
        if let Some(pos) = boot_dev_str.rfind(|c: char| c.is_ascii_digit()) {
            // Find the last digit and remove everything from there
            let mut disk = boot_dev_str[..pos].to_string();
            // Handle special case for nvme devices (e.g., /dev/nvme0n1p3 -> /dev/nvme0n1)
            if disk.ends_with('p') {
                disk.pop(); // Remove the 'p'
            }
            Ok(PathBuf::from(disk))
        } else {
            Ok(boot_dev.to_path_buf())
        }
    }

    /// Detect partition table type by using fdisk -l command
    async fn detect_partition_table_type(&self, disk_dev: &Path) -> Result<PartitionTableType> {
        // Run fdisk -l command to get disk information
        let stdout = Command::new("fdisk")
            .args(["-l", &disk_dev.to_string_lossy()])
            .run()
            .await
            .with_context(|| format!("Failed to execute fdisk -l for {:?}", disk_dev))?;

        let stdout_str = String::from_utf8_lossy(&stdout);

        // Parse the output to determine partition table type
        // Look for lines that indicate the partition table type
        for line in stdout_str.lines() {
            if line.contains("Disklabel type: gpt") {
                return Ok(PartitionTableType::Gpt);
            } else if line.contains("Disklabel type: dos") {
                return Ok(PartitionTableType::Mbr);
            }
        }

        // If we can't determine the type from the output, default to GPT
        tracing::warn!(
            "Cannot determine partition table type for {:?} from fdisk output, defaulting to GPT",
            disk_dev
        );
        Ok(PartitionTableType::Gpt)
    }

    fn get_boot_dev(&self) -> &Path;

    async fn read_file_on_disk_to_string(&self, path: &Path) -> Result<String> {
        self.read_file_on_disk(path)
            .await
            .and_then(|v| anyhow::Ok(String::from_utf8(v)?))
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>>;

    /// Read GRUB and SHIM EFI binaries
    async fn read_grub_and_shim(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        // Walk and search for grubx64.efi and shimx64.efi from /boot/efi
        let efi_part_root_dir = self.get_efi_part_root_dir();

        let mut grub_data: Option<Vec<u8>> = None;
        let mut shim_data: Option<Vec<u8>> = None;

        let mut entries = async_walkdir::WalkDir::new(efi_part_root_dir);

        loop {
            match entries.next().await {
                Some(Ok(entry)) => {
                    if matches!(entry.file_type().await.map(|e| e.is_file()), Ok(true)) {
                        let file_name = entry.file_name().to_string_lossy().to_lowercase();

                        if file_name == "grubx64.efi" && grub_data.is_none() {
                            tracing::debug!(file=?entry.path(), "Found grubx64.efi");
                            let mut buf = vec![];
                            let mut file = File::open(entry.path()).await?;
                            file.read_to_end(&mut buf).await?;
                            grub_data = Some(buf);
                        } else if file_name == "shimx64.efi" && shim_data.is_none() {
                            tracing::debug!(file=?entry.path(), "Found shimx64.efi");
                            let mut buf = vec![];
                            let mut file = File::open(entry.path()).await?;
                            file.read_to_end(&mut buf).await?;
                            shim_data = Some(buf);
                        }

                        // If we found both files, we can stop searching
                        if grub_data.is_some() && shim_data.is_some() {
                            break;
                        }
                    }
                }
                Some(Err(_)) | None => {
                    break;
                }
            }
        }

        match (grub_data, shim_data) {
            (Some(grub), Some(shim)) => Ok((grub, shim)),
            (None, _) => bail!("GRUB EFI binary (grubx64.efi) not found in /boot/efi"),
            (_, None) => bail!("SHIM EFI binary (shimx64.efi) not found in /boot/efi"),
        }
    }

    fn get_efi_part_root_dir(&self) -> &Path;
}

const CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT: &'static str = "cryptpilot/config";
const METADATA_PATH_IN_BOOT: &'static str = "cryptpilot/metadata.toml";

async fn load_fde_config_bundle_from_dir(config_dir: &Path) -> Result<FdeConfigBundle> {
    Ok(FileSystemConfigSource::new(config_dir)
        .get_config()
        .await
        .with_context(|| format!("Can not read config dir at {config_dir:?}"))?
        .strip_as_fde_config_bundle())
}

async fn load_metadata_from_file(metadata_path: &Path) -> Result<Metadata> {
    let metadata_content = tokio::fs::read_to_string(&metadata_path)
        .await
        .with_context(|| format!("Can not read metadata file at {metadata_path:?}"))?;
    let mut metadata = toml::from_str::<Metadata>(&metadata_content)?;

    tracing::debug!("Metadata content:\n{}", metadata_content);

    // Sanity check on root_hash, since it is from unsafe source
    let root_hash_bin = hex::decode(metadata.root_hash).context("Bad root hash")?;
    metadata.root_hash = hex::encode(root_hash_bin);

    Ok(metadata)
}

/// Load the fde related config bundle from current system. This should be used
/// only when the system is booted into the system manager (systemd) stage, and
/// should not be used in initrd stage.
#[non_exhaustive]
pub struct OnCurrentSystemFdeDisk {
    boot_dev: PathBuf,
}

impl OnCurrentSystemFdeDisk {
    pub async fn new() -> Result<Self> {
        if !Path::new("/boot").join(METADATA_PATH_IN_BOOT).exists() {
            bail!("Metadata file not found in /boot.\nThe current system may not be booted from an encrypted disk. You can follow the instructions here to it encrypt it first: https://github.com/openanolis/cryptpilot")
        }

        // Find the block device that contains /boot mount point
        let boot_dev = async {
            let mut cmd = Command::new("findmnt");
            cmd.args(["-n", "-o", "SOURCE", "/boot"]);

            let stdout = cmd.run().await?;
            let boot_dev = PathBuf::from(String::from_utf8(stdout)?.trim().to_string());

            if !boot_dev.exists() {
                bail!("Boot partition not exists");
            }
            Ok(boot_dev)
        }
        .await
        .context("Failed to determine /boot mount source")?;

        Ok(Self { boot_dev })
    }
}

#[async_trait]
impl FdeDisk for OnCurrentSystemFdeDisk {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        load_fde_config_bundle_from_dir(
            &Path::new("/boot").join(CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT),
        )
        .await
    }

    async fn load_metadata(&self) -> Result<Metadata> {
        load_metadata_from_file(&Path::new("/boot").join(METADATA_PATH_IN_BOOT)).await
    }

    async fn get_grub_env(&self) -> Result<String> {
        // Get the saved entry from GRUB environment
        let stdout = Command::new("grub2-editenv").arg("list").run().await?;

        let grub_env = String::from_utf8(stdout)?;

        Ok(grub_env)
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>> {
        let mut file = File::open(path).await?;
        let mut buf = vec![];
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    fn get_boot_dev(&self) -> &Path {
        &self.boot_dev
    }

    fn get_efi_part_root_dir(&self) -> &Path {
        Path::new("/boot/efi")
    }
}

/// Load the fde related config bundle from a disk device.
pub struct OnExternalFdeDisk {
    #[allow(unused)]
    nbd_device: Option<NbdDevice>,
    boot_dev: PathBuf,
    boot_dev_tmp_mount: TmpMountPoint,
    #[allow(unused)]
    efi_dev: PathBuf,
    efi_dev_tmp_mount: TmpMountPoint,
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

        // Find the boot partition and mount it to a tmp mount point
        let boot_dev = Self::detect_boot_part(Some(&disk_device)).await.context(
            "Cannot found boot partition on the disk. The disk may not be a encrypted disk.",
        )?;
        let boot_dev_tmp_mount = TmpMountPoint::mount(&boot_dev).await?;

        // Find the EFI partition and mount it to a tmp mount point
        let efi_dev = Self::detect_efi_part(Some(&disk_device)).await.context(
            "Cannot found EFI partition on the disk. The disk may not be a encrypted disk.",
        )?;
        let efi_dev_tmp_mount = TmpMountPoint::mount(&efi_dev).await?;

        Ok(Self {
            nbd_device,
            boot_dev,
            boot_dev_tmp_mount,
            efi_dev,
            efi_dev_tmp_mount,
        })
    }

    /// New by probing the boot partition on current environment. This is used in initrd stage.
    pub async fn new_by_probing() -> Result<Self> {
        let boot_dev = Self::detect_boot_part(None).await?;
        let boot_dev_tmp_mount = TmpMountPoint::mount(&boot_dev).await?;
        let efi_dev = Self::detect_efi_part(None).await?;
        let efi_dev_tmp_mount = TmpMountPoint::mount(&efi_dev).await?;

        Ok(Self {
            nbd_device: None,
            boot_dev,
            boot_dev_tmp_mount,
            efi_dev,
            efi_dev_tmp_mount,
        })
    }

    pub async fn detect_boot_part(hint_device: Option<&Path>) -> Result<PathBuf> {
        // 1. Execute 'findmnt-n-o SOURCE /boot' to return the device path where '/boot' is mounted
        let mut command = Command::new("findmnt");
        command.args(["-n", "-o", "SOURCE", "/boot"]);
        match command.run().await {
            Ok(stdout) => {
                let stdout_str = String::from_utf8_lossy(&stdout).trim().to_string();
                if !stdout_str.is_empty() {
                    // Return the device path, such as /dev/sda1
                    return Ok(PathBuf::from(stdout_str));
                }
            }
            Err(e) => {
                tracing::warn!("findmnt failed: {}", e);
            }
        }

        // 2. Try GPT-style PARTLABEL match
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

        if let Some(hint_device) = hint_device {
            gpt_cmd.arg(hint_device);
        }

        let gpt_result = gpt_cmd.output().await?;
        let gpt_device = String::from_utf8_lossy(&gpt_result.stdout)
            .trim()
            .to_string();

        if !gpt_device.is_empty() {
            return Ok(PathBuf::from(gpt_device));
        }

        // 3. Try MBR-style fallback: search all ext4 partitions and check contents
        let output = Command::new("lsblk")
            .args(["-lnpo", "NAME,FSTYPE"])
            .run()
            .await
            .context("lsblk failed")?;

        let content = String::from_utf8_lossy(&output);
        for line in content.lines() {
            let fields: Vec<&str> = line.trim().split_whitespace().collect();
            if fields.len() != 2 {
                continue;
            }

            let dev = fields[0];

            // Try mounting and checking for boot content
            let tmpdir = tempdir().context("Failed to create temp mount dir")?;
            let mount_path = tmpdir.path();
            let mount_str = mount_path
                .to_str()
                .ok_or_else(|| anyhow!("Invalid mount path: non-UTF8"))?;

            let already_mounted = Command::new("findmnt")
                .args(["-n", "-o", "TARGET", dev])
                .run()
                .await
                .is_ok();

            if !already_mounted {
                if Command::new("mount")
                    .args(["-o", "ro", dev, mount_str])
                    .run()
                    .await
                    .is_err()
                {
                    continue;
                }
            }

            let mut has_boot_kernel = false;

            let mut entries = fs::read_dir(mount_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("vmlinuz") {
                    has_boot_kernel = true;
                    break;
                }
            }

            // Unmount after check
            let _ = Command::new("umount").arg(mount_path).status().await;

            if has_boot_kernel {
                return Ok(PathBuf::from(dev));
            }
        }

        bail!("No boot partition found (GPT and MBR methods both failed)");
    }

    async fn detect_efi_part(hint_device: Option<&PathBuf>) -> Result<PathBuf> {
        let mut cmd = Command::new("blkid");
        cmd.args(["--match-types", "vfat"])
            .args(["--match-token", r#"PARTLABEL="EFI System Partition""#])
            .args(["--list-one", "--output", "device"]);

        if let Some(hint_device) = hint_device {
            cmd.arg(hint_device);
        };

        cmd.run()
            .await
            .and_then(|stdout| {
                let mut device_name = String::from_utf8(stdout)?;
                device_name = device_name.trim().into();
                if device_name.is_empty() {
                    bail!("No EFI partition found");
                }
                Ok(PathBuf::from(device_name))
            })
            .context("Failed to detect EFI partition")
    }
}

#[async_trait]
impl FdeDisk for OnExternalFdeDisk {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        let config_dir = self
            .boot_dev_tmp_mount
            .mount_point()
            .join(CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT);
        if !config_dir.exists() {
            bail!("No config dir found in boot partition. The disk may not be a encrypted disk.")
        }
        load_fde_config_bundle_from_dir(&config_dir).await
    }

    async fn load_metadata(&self) -> Result<Metadata> {
        let metadata_file = self
            .boot_dev_tmp_mount
            .mount_point()
            .join(METADATA_PATH_IN_BOOT);

        if !metadata_file.exists() {
            bail!("No metadata file found in boot partition. The disk may not be a encrypted disk.")
        }

        load_metadata_from_file(&metadata_file).await
    }

    async fn get_grub_env(&self) -> Result<String> {
        // Try to find the GRUB environment file
        let mount_point = self.boot_dev_tmp_mount.mount_point();
        let mut grub_env_path = mount_point.join("grubenv");

        // If grubenv doesn't exist, try grub/grubenv
        if !grub_env_path.exists() {
            grub_env_path = mount_point.join("grub/grubenv");
        }

        if !grub_env_path.exists() {
            grub_env_path = mount_point.join("grub2/grubenv");
        }

        // Read GRUB environment
        let grub_env_content = tokio::fs::read_to_string(&grub_env_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to read GRUB environment file at {:?}",
                    grub_env_path
                )
            })?;

        Ok(grub_env_content)
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>> {
        if !path.starts_with("/boot") {
            bail!("The path must be start with /boot")
        }
        let path = self
            .boot_dev_tmp_mount
            .mount_point()
            .join(path.strip_prefix("/boot")?);

        let mut file = File::open(path).await?;
        let mut buf = vec![];
        file.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    fn get_boot_dev(&self) -> &Path {
        &self.boot_dev
    }

    fn get_efi_part_root_dir(&self) -> &Path {
        self.efi_dev_tmp_mount.mount_point()
    }
}

fn parse_pe(bytes: &[u8]) -> Result<Box<dyn PeTrait + '_>, object::read::Error> {
    if let Ok(pe) = PeFile64::parse(bytes) {
        Ok(Box::new(pe))
    } else {
        let pe = PeFile32::parse(bytes)?;
        Ok(Box::new(pe))
    }
}
