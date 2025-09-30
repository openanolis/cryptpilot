use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use async_walkdir::WalkDir;
use authenticode::PeTrait;
use block_devs::BlckExt;
use futures_lite::stream::StreamExt;
use object::read::pe::{PeFile32, PeFile64};
use sha2::Digest;
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

#[derive(Debug)]
pub struct MeasurementedBootComponents(pub Vec<(GrubArtifacts, KernelArtifacts)>);

impl MeasurementedBootComponents {
    pub fn cal_hash<T>(&self) -> Result<BootComponentsHashValues>
    where
        T: digest::Digest + digest::Update,
    {
        let kernel_cmdline_hashs = self
            .0
            .iter()
            .map(|(_, kernel_artifacts)| {
                let mut hasher = T::new();
                Digest::update(&mut hasher, &kernel_artifacts.kernel_cmdline);
                hex::encode(hasher.finalize())
            })
            .collect::<Vec<_>>();

        let kernel_hashs = self
            .0
            .iter()
            .map(|(_, kernel_artifacts)| {
                let mut hasher = T::new();
                Digest::update(&mut hasher, &kernel_artifacts.kernel);
                hex::encode(hasher.finalize())
            })
            .collect::<Vec<_>>();

        let initrd_hashs = self
            .0
            .iter()
            .map(|(_, kernel_artifacts)| {
                let mut hasher = T::new();
                Digest::update(&mut hasher, &kernel_artifacts.initrd);
                hex::encode(hasher.finalize())
            })
            .collect::<Vec<_>>();

        let grub_authenticode_hashes = self
            .0
            .iter()
            .map(|(grub_artifacts, _)| calculate_authenticode_hash::<T>(&grub_artifacts.grub_data))
            .collect::<Result<Vec<_>>>()?;

        let shim_authenticode_hashes = self
            .0
            .iter()
            .map(|(grub_artifacts, _)| calculate_authenticode_hash::<T>(&grub_artifacts.shim_data))
            .collect::<Result<Vec<_>>>()?;

        Ok(BootComponentsHashValues {
            kernel_cmdline_hashs,
            kernel_hashs,
            initrd_hashs,
            grub_authenticode_hashes,
            shim_authenticode_hashes,
        })
    }
}

/// Structure to hold kernel and initrd information
#[derive(Debug, Clone)]
pub struct BootComponentsHashValues {
    pub kernel_cmdline_hashs: Vec<String>,
    pub kernel_hashs: Vec<String>,
    pub initrd_hashs: Vec<String>,
    pub grub_authenticode_hashes: Vec<String>,
    pub shim_authenticode_hashes: Vec<String>,
}

#[async_trait]
pub trait FdeDisk: Send + Sync {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle>;

    async fn load_metadata(&self) -> Result<Metadata>;

    async fn get_boot_components(&self) -> Result<MeasurementedBootComponents> {
        let mut components = vec![];

        tracing::debug!("Try to load grub.cfg file from BOOT partition");
        let global_grub_env = match self.load_global_grub_env_file().await {
            Ok(v) => Some(v),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "No grub.cfg file found in BOOT partition, fallback to search grub.cfg file from EFI partition"
                );
                None
            }
        };

        tracing::debug!("Try to load grubenv file from BOOT partition");
        let global_grub_cfg = match self.load_global_grub_cfg_file().await {
            Ok(v) => Some(v),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "No grubenv file found in BOOT partition, fallback to search grubenv file from EFI partition"
                );
                None
            }
        };

        let grub_artifacts = self.load_grub_artifacts().await?;

        for grub_artifact in grub_artifacts {
            let Some(grub_env) = global_grub_env
                .as_deref()
                .or(grub_artifact.grub_env.as_deref())
            else {
                tracing::warn!(
                    dir = ?grub_artifact.efi_grub_dir,
                    "No grubenv file found, skip this grub directory"
                );
                continue;
            };

            let Some(grub_cfg) = global_grub_cfg
                .as_deref()
                .or(grub_artifact.grub_cfg.as_deref())
            else {
                tracing::warn!(
                    dir = ?grub_artifact.efi_grub_dir,
                    "No grub.cfg file found, skip this grub directory"
                );
                continue;
            };

            let grub_vars = self.parse_grub_env_vars(grub_env, grub_cfg).await?;

            let kernel_artifacts = self.load_kernel_artifacts(&grub_vars, grub_cfg).await?;
            components.push((grub_artifact, kernel_artifacts))
        }

        if components.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to calculate reference value for any GRUB components"
            ));
        }

        Ok(MeasurementedBootComponents(components))
    }

    async fn parse_grub_env_vars(
        &self,
        grub_env: &str,
        grub_cfg: &str,
    ) -> Result<HashMap<String, String>> {
        // Parse GRUB environment variables
        let mut grub_vars = HashMap::new();
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
            // Look for kernelopts definition in the GRUB config
            for line in grub_cfg.lines() {
                if line.contains("set kernelopts=") {
                    if let Some(opts) = line.strip_prefix("set kernelopts=") {
                        let opts_value = opts.trim().trim_matches('"').to_string();
                        grub_vars.insert("kernelopts".to_string(), opts_value);
                        break;
                    }
                }
            }

            // If still not found, look for fallback kernelopts definition
            let lines: Vec<&str> = grub_cfg.lines().collect();
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

        Ok(grub_vars)
    }

    async fn load_from_loader_entry_file(
        &self,
        saved_entry: &str,
        grub_vars: &HashMap<String, String>,
    ) -> Result<(PathBuf, PathBuf, String)> {
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
                kernel_path = line
                    .split_once(' ')
                    .map(|x| x.1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            } else if line.starts_with("options ") {
                cmdline = line
                    .split_once(' ')
                    .map(|x| x.1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            } else if line.starts_with("initrd ") {
                initrd_path = line
                    .split_once(' ')
                    .map(|x| x.1)
                    .unwrap_or("")
                    .trim()
                    .to_string();
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

        Ok((
            PathBuf::from(kernel_path),
            PathBuf::from(initrd_path),
            cmdline,
        ))
    }

    async fn load_from_grub_cfg(
        &self,
        saved_entry: &str,
        grub_cfg: &str,
    ) -> Result<(PathBuf, PathBuf, String)> {
        // Find the menuentry that matches the saved_entry
        let mut in_target_entry = false;
        let mut kernel_line = None;
        let mut initrd_line = None;

        for line in grub_cfg.lines() {
            let line = line.trim();

            // Check if we're entering the target menuentry
            if line.starts_with("menuentry") && line.contains(saved_entry) {
                in_target_entry = true;
                continue;
            }

            // Check if we're leaving the current menuentry
            if in_target_entry && line == "}" {
                break;
            }

            // Process lines within the target menuentry
            if in_target_entry {
                if line.starts_with("linuxefi") {
                    kernel_line = Some(line.to_string());
                } else if line.starts_with("initrdefi") {
                    initrd_line = Some(line.to_string());
                }
            }
        }

        // Extract kernel path and additional parameters
        let mut kernel_path = String::new();
        let mut cmdline = String::new();

        if let Some(kernel_line) = kernel_line {
            let parts: Vec<&str> = kernel_line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                kernel_path = parts[1].to_string();

                // Extract command line parameters if present
                if let Some(space_pos) = kernel_path.find(' ') {
                    cmdline = kernel_path[space_pos + 1..].to_string();
                    kernel_path = kernel_path[..space_pos].to_string();
                }
            }
        }

        if let Some(path) = kernel_path.strip_prefix('/') {
            if !kernel_path.starts_with("/boot") {
                kernel_path = format!("/boot/{path}");
            }
        }

        // Extract initrd path
        let mut initrd_path = String::new();
        if let Some(initrd_line) = initrd_line {
            let parts: Vec<&str> = initrd_line.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                initrd_path = parts[1].to_string();
                // Clean up if there's extra content
                if let Some(space_pos) = initrd_path.find(' ') {
                    initrd_path = initrd_path[..space_pos].to_string();
                }
            }
        }

        if let Some(path) = initrd_path.strip_prefix('/') {
            if !initrd_path.starts_with("/boot") {
                initrd_path = format!("/boot/{path}");
            }
        }

        cmdline = cmdline.replace("  ", " ");
        cmdline = cmdline.trim().to_string();

        Ok((
            PathBuf::from(kernel_path),
            PathBuf::from(initrd_path),
            cmdline,
        ))
    }

    async fn load_kernel_artifacts(
        &self,
        grub_vars: &HashMap<String, String>,
        grub_cfg: &str,
    ) -> Result<KernelArtifacts> {
        let saved_entry = grub_vars
            .get("saved_entry")
            .ok_or_else(|| anyhow::anyhow!("saved_entry not found in GRUB environment"))?;

        let (mut kernel_path, mut initrd_path, cmdline) = match self
            .load_from_loader_entry_file(saved_entry, grub_vars)
            .await
        {
            Ok(v) => v,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "Failed to parse kernel artifacts info from loader entry file, fallback to parse from grub.cfg"
                );

                self.load_from_grub_cfg(saved_entry, grub_cfg).await?
            }
        };

        // Make kernel path absolute if needed
        if kernel_path.is_relative() {
            kernel_path = Path::new("/boot").join(kernel_path);
        }

        // Make initrd path absolute if needed
        if initrd_path.is_relative() {
            initrd_path = Path::new("/boot").join(initrd_path);
        }

        let kernel_path = Path::new(&kernel_path);

        // Calculate SHA384 hashes
        let kernel = self
            .read_file_on_disk(kernel_path)
            .await
            .with_context(|| format!("Failed to read kernel file at {:?}", kernel_path))?;

        let initrd = self
            .read_file_on_disk(Path::new(&initrd_path))
            .await
            .with_context(|| format!("Failed to read initrd file at {:?}", initrd_path))?;

        // Construct full kernel command line with inferred device identifier prefix
        let full_kernel_cmdline = {
            // Infer device identifier from current boot partition device path
            let device_identifier = {
                // Detect partition table type
                let partition_type = self.detect_disk_type().await?;

                // Extract partition number from device path
                // For example, /dev/sda3 -> (hd0,gpt3) or (hd0,msdos3), /dev/nvme0n1p3 -> (hd0,gpt3) or (hd0,msdos3)
                let boot_dev = self.get_boot_dev();
                if let Ok(partition_num) = boot_dev
                    .to_string_lossy()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
                    .parse::<u32>()
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
                kernel_path.to_string_lossy(),
                cmdline
            )
        };

        Ok(KernelArtifacts {
            kernel_cmdline: full_kernel_cmdline,
            kernel,
            initrd,
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

    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool>;

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>>;

    /// Read grub related artifacts
    ///
    /// Find directories containing grubx64.efi, then read all required files from that same directory.
    async fn load_grub_artifacts(&self) -> Result<Vec<GrubArtifacts>> {
        let efi_part_root_dir = self.get_efi_part_root_dir();
        let mut artifacts_list = vec![];

        let mut entries = WalkDir::new(efi_part_root_dir);
        let mut grub_dirs = HashSet::new();

        // Step 1: Collect all directories containing 'grubx64.efi'
        while let Some(Ok(entry)) = entries.next().await {
            if entry.file_type().await.map_or(false, |ft| ft.is_file()) {
                let file_name = entry.file_name().to_string_lossy().to_lowercase();
                if file_name == "grubx64.efi" {
                    let parent_dir = entry.path().parent().map(|p| p.to_path_buf());
                    if let Some(dir) = parent_dir {
                        tracing::debug!(dir = ?dir, "Found grubx64.efi, will scan this directory");
                        grub_dirs.insert(dir);
                    }
                }
            }
        }

        if grub_dirs.is_empty() {
            bail!("No grubx64.efi found under {}", efi_part_root_dir.display());
        }

        // Step 2: For each such directory, try to read all required artifacts
        for dir in grub_dirs {
            let mut grub_data = None;
            let mut shim_data = None;
            let mut grub_env = None;
            let mut grub_cfg = None;

            // List all files in this GRUB directory
            let mut dir_entries = WalkDir::new(&dir);
            while let Some(Ok(inner_entry)) = dir_entries.next().await {
                if !inner_entry
                    .file_type()
                    .await
                    .map_or(false, |ft| ft.is_file())
                {
                    continue;
                }

                let file_path = inner_entry.path();
                let file_name = match file_path.file_name() {
                    Some(file_name) => file_name.to_string_lossy().to_lowercase(),
                    None => continue,
                };

                match file_name.as_str() {
                    "grubx64.efi" => {
                        tracing::debug!(file = ?file_path, "Reading grubx64.efi");
                        let mut buf = Vec::new();
                        File::open(file_path).await?.read_to_end(&mut buf).await?;
                        grub_data = Some(buf);
                    }
                    "shimx64.efi" | "shim.efi" => {
                        tracing::debug!(file = ?file_path, "Reading grub shim");
                        let mut buf = Vec::new();
                        File::open(file_path).await?.read_to_end(&mut buf).await?;
                        shim_data = Some(buf);
                    }
                    "grubenv" => {
                        tracing::debug!(file = ?file_path, "Reading grubenv");
                        grub_env = Some(tokio::fs::read_to_string(file_path).await?);
                    }
                    "grub.cfg" => {
                        tracing::debug!(file = ?file_path, "Reading grub.cfg");
                        grub_cfg = Some(tokio::fs::read_to_string(file_path).await?);
                    }
                    _ => {}
                }
            }

            // Validate required binaries are present
            let Some(grub_data) = grub_data else {
                tracing::warn!(dir = ?dir, "Missing grubx64.efi in directory, skipping");
                continue;
            };

            let Some(shim_data) = shim_data else {
                tracing::warn!(dir = ?dir, "Missing shimx64.efi in directory, skipping");
                continue;
            };

            artifacts_list.push(GrubArtifacts {
                efi_grub_dir: dir,
                grub_data,
                shim_data,
                grub_env,
                grub_cfg,
            });
        }

        if artifacts_list.is_empty() {
            bail!("Found grubx64.efi directories but failed to load complete artifacts from any");
        }

        Ok(artifacts_list)
    }

    fn get_efi_part_root_dir(&self) -> &Path;

    async fn load_global_grub_env_file(&self) -> Result<String>;

    async fn load_global_grub_cfg_file(&self) -> Result<String> {
        let grub_cfg_path = Path::new("/boot/grub2/grub.cfg");

        // Read GRUB environment
        let grub_cfg_content = self
            .read_file_on_disk(grub_cfg_path)
            .await
            .with_context(|| format!("Failed to read GRUB config file at {:?}", grub_cfg_path))?;

        Ok(String::from_utf8(grub_cfg_content)?)
    }
}

/// Represents all GRUB-related artifacts found in the same directory as grubx64.efi.
#[derive(Debug)]
pub struct GrubArtifacts {
    pub efi_grub_dir: PathBuf,
    pub grub_data: Vec<u8>,
    pub shim_data: Vec<u8>,
    pub grub_env: Option<String>,
    pub grub_cfg: Option<String>,
}

#[derive(Debug)]
pub struct KernelArtifacts {
    pub kernel_cmdline: String,
    pub kernel: Vec<u8>,
    pub initrd: Vec<u8>,
}

const CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT: &str = "cryptpilot/config";
const METADATA_PATH_IN_BOOT: &str = "cryptpilot/metadata.toml";

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

    async fn load_global_grub_env_file(&self) -> Result<String> {
        // Get the saved entry from GRUB environment
        let stdout = Command::new("grub2-editenv").arg("list").run().await?;

        let grub_env = String::from_utf8(stdout)?;

        Ok(grub_env)
    }

    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool> {
        Ok(path.exists())
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
        if hint_device.is_none() && Command::new("mountpoint").arg("/boot").run().await.is_ok() {
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
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "Failed to find boot partition from /boot mount point"
                    );
                }
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

        // 3. Try MBR-style fallback: search all ext4 partitions and check contents
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
                let tmp_mount = TmpMountPoint::mount(&dev).await?;

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

    async fn detect_efi_part(hint_device: Option<&PathBuf>) -> Result<PathBuf> {
        // Obtain all partitions under the device
        let lsblk_stdout = {
            let mut cmd = Command::new("lsblk");
            cmd.args(["-lnpo", "NAME"]);
            if let Some(device) = hint_device {
                cmd.arg(device);
            }
            cmd.run().await.context("Failed to list partitions")?
        };

        let lsblk_str = String::from_utf8(lsblk_stdout)?;
        let partitions = lsblk_str
            .lines()
            .filter(|line| line.chars().last().map(|c| c.is_numeric()).unwrap_or(false))
            .map(PathBuf::from);

        for part in partitions {
            let is_efi_part = async {
                // Create a temporary mount point
                let tmp_mount = TmpMountPoint::mount(&part).await?;
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

            let is_efi_part = match is_efi_part {
                Ok(is_efi_part) => is_efi_part,
                Err(error) => {
                    tracing::debug!(?error, ?part, "Failed to check efi part on device");
                    continue;
                }
            };

            if is_efi_part {
                return Ok(part);
            }
        }

        bail!("No valid EFI partition found");
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

    fn check_file_exist_on_disk(&self, path: &Path) -> Result<bool> {
        if !path.starts_with("/boot") {
            bail!("The path must be start with /boot")
        }
        let real_path = if path.starts_with("/boot/efi") {
            self.efi_dev_tmp_mount
                .mount_point()
                .join(path.strip_prefix("/boot/efi")?)
        } else {
            self.boot_dev_tmp_mount
                .mount_point()
                .join(path.strip_prefix("/boot")?)
        };

        Ok(real_path.exists())
    }

    async fn read_file_on_disk(&self, path: &Path) -> Result<Vec<u8>> {
        let real_path = {
            let mut path = path.to_path_buf();
            loop {
                if !path.starts_with("/boot") {
                    bail!("The path must be start with /boot but got {path:?}")
                }

                let real_path = if path.starts_with("/boot/efi") {
                    self.efi_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/boot/efi")?)
                } else {
                    self.boot_dev_tmp_mount
                        .mount_point()
                        .join(path.strip_prefix("/boot")?)
                };

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

fn calculate_authenticode_hash<T: digest::Digest + digest::Update>(bytes: &[u8]) -> Result<String> {
    let pe = parse_pe(bytes)?;
    let mut hasher = T::new();
    authenticode::authenticode_digest(&*pe, &mut hasher)
        .context("calculate_authenticode_hash failed")?;
    Ok(hex::encode(hasher.finalize()))
}
