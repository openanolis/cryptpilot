use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use async_walkdir::WalkDir;
use authenticode::PeTrait;
use futures::StreamExt;
use indexmap::IndexMap;
use object::read::pe::{PeFile32, PeFile64};
use tokio::{fs::File, io::AsyncReadExt as _};

use crate::cmd::fde::disk::{
    artifacts::BootArtifacts, kernel::KernelArtifacts, Disk, PartitionTableType,
};

/// Represents all GRUB-related artifacts found in the same directory as grubx64.efi.
/// This includes the GRUB binary, configuration files, associated shim binary, and environment data.
/// These artifacts are typically used during the UEFI boot process to load the operating system kernel.
#[derive(Debug)]
pub struct GrubArtifacts {
    /// The directory path containing the GRUB EFI binary (e.g., containing grubx64.efi).
    pub efi_grub_dir: PathBuf,

    /// Raw byte content of the GRUB binary (usually grubx64.efi).
    pub grub_data: Vec<u8>,

    /// Raw byte content of the Shim binary (usually shimx64.efi), used for secure boot.
    pub shim_data: Vec<u8>,

    /// Optional contents of the GRUB environment block file (e.g., grubenv).
    /// This file may store persistent boot variables such as the current boot entry.
    pub grub_env: Option<String>,

    /// Optional contents of the main GRUB configuration file (grub.cfg).
    /// This file defines menu entries and kernel boot parameters.
    pub grub_cfg: Option<String>,
}

#[derive(Debug)]
pub struct GrubBootArtifactsItem {
    grub: GrubArtifacts,
    kernel: KernelArtifacts,
}

pub type GrubBootArtifacts = Vec<GrubBootArtifactsItem>;

#[async_trait]
impl BootArtifacts for GrubBootArtifacts {
    async fn inseart_reference_value<T>(
        &self,
        map: &mut IndexMap<String, Vec<String>>,
        hash_key: &str,
    ) -> Result<()>
    where
        T: digest::Digest + digest::Update,
    {
        map.insert(
            "kernel_cmdline".to_string(),
            self.iter()
                .flat_map(|GrubBootArtifactsItem { grub: _, kernel }| {
                    kernel
                        .kernel_cmdlines
                        .iter()
                        .map(|cmdline| format!("grub_kernel_cmdline {}", cmdline))
                })
                .collect::<Vec<_>>(),
        );

        map.insert(
            format!("measurement.kernel_cmdline.{hash_key}"),
            self.iter()
                .flat_map(|GrubBootArtifactsItem { grub: _, kernel }| {
                    kernel.kernel_cmdlines.iter().map(|cmdline| {
                        let mut hasher = T::new();
                        digest::Digest::update(&mut hasher, cmdline);
                        hex::encode(hasher.finalize())
                    })
                })
                .collect::<Vec<_>>(),
        );

        map.insert(
            format!("measurement.kernel.{hash_key}"),
            self.iter()
                .map(|GrubBootArtifactsItem { grub: _, kernel }| {
                    let mut hasher = T::new();
                    digest::Digest::update(&mut hasher, &kernel.kernel);
                    hex::encode(hasher.finalize())
                })
                .collect::<Vec<_>>(),
        );

        map.insert(
            format!("measurement.initrd.{hash_key}"),
            self.iter()
                .map(|GrubBootArtifactsItem { grub: _, kernel }| {
                    let mut hasher = T::new();
                    digest::Digest::update(&mut hasher, &kernel.initrd);
                    hex::encode(hasher.finalize())
                })
                .collect::<Vec<_>>(),
        );

        map.insert(
            format!("measurement.grub.{hash_key}"),
            self.iter()
                .map(|GrubBootArtifactsItem { grub, kernel: _ }| {
                    calculate_authenticode_hash::<T>(&grub.grub_data)
                })
                .collect::<Result<Vec<_>>>()?,
        );

        map.insert(
            format!("measurement.shim.{hash_key}"),
            self.iter()
                .map(|GrubBootArtifactsItem { grub, kernel: _ }| {
                    calculate_authenticode_hash::<T>(&grub.shim_data)
                })
                .collect::<Result<Vec<_>>>()?,
        );

        Ok(())
    }

    async fn extract_kernel_artifacts(&self) -> Result<Vec<KernelArtifacts>> {
        Ok(self
            .iter()
            .map(|GrubBootArtifactsItem { grub: _, kernel }| kernel.to_owned())
            .collect())
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

pub async fn parse_grub_env_vars(
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

#[async_trait]
pub(super) trait FdeDiskGrubExt: Disk {
    async fn extract_boot_artifacts_grub(&self) -> Result<GrubBootArtifacts> {
        let mut artifacts: Vec<_> = vec![];

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

            let grub_vars = parse_grub_env_vars(grub_env, grub_cfg).await?;

            let kernel_artifacts = self.load_kernel_artifacts(&grub_vars, grub_cfg).await?;
            artifacts.push(GrubBootArtifactsItem {
                grub: grub_artifact,
                kernel: kernel_artifacts,
            })
        }

        if artifacts.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to calculate reference value for any GRUB artifacts"
            ));
        }

        Ok(artifacts)
    }

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

        // Generate a kernel command line that omits the device identifier prefix (e.g., "/vmlinuz-5.10.134-19.1.al8.x86_64 root=UUID=2576d86b-4895-4922-b9d9-7c89dec6caa9 ro crashkernel=auto console=ttyS0,115200 nokaslr").
        // This format is typically used when GRUB sets the root device via `--set=root`, allowing the kernel path to be relative to the boot partition.
        let full_kernel_cmdline_shorter = {
            let kernel_path_string = kernel_path.to_string_lossy();
            let kernel_path_in_boot_dir = kernel_path_string
                .strip_prefix("/boot")
                .unwrap_or(&kernel_path_string);
            format!("{} {}", kernel_path_in_boot_dir, cmdline)
        };

        // Construct a full kernel command line that includes an inferred device identifier prefix (e.g., "(hd0,gpt2)/vmlinuz-... root=...").
        // This format is used when GRUB does not rely on `--set=root` and instead embeds the full device path to locate the kernel.
        let full_kernel_cmdline_with_device_identifier = {
            // Infer device identifier from current boot partition device path
            let device_identifier = {
                // Detect partition table type
                let partition_type = self.detect_disk_partition_type().await?;

                // Extract partition number from device path
                // For example, /dev/sda3 -> (hd0,gpt3) or (hd0,msdos3), /dev/nvme0n1p3 -> (hd0,gpt3) or (hd0,msdos3)
                let boot_dir_dev = self.get_boot_dir_located_dev()?;
                if let Ok(partition_num) = boot_dir_dev
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
                        "Unable to extract partition number from device path: {:?}",
                        boot_dir_dev
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
            kernel_cmdlines: vec![
                full_kernel_cmdline_shorter,
                full_kernel_cmdline_with_device_identifier,
            ],
            kernel,
            initrd,
        })
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
