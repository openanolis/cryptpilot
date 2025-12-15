use anyhow::{Context as _, Result};
use std::path::Path;
use tempfile::TempDir;
use tokio::process::Command;

use crate::{
    cmd::boot_service::{
        metadata::{load_metadata_from_file, Metadata},
        stage::before_sysroot::METADATA_PATH_IN_INITRD,
    },
    config::source::{
        cloud_init::FdeConfigBundle,
        fs::{FileSystemConfigSource, CRYPTPILOT_CONFIG_DIR_DEFAULT},
        ConfigSource,
    },
    fs::cmd::CheckCommandOutput,
};

/// Represents kernel and initrd images along with command line arguments needed to boot the OS.
#[derive(Debug)]
pub struct KernelArtifacts {
    /// A list of kernel command line arguments passed to the kernel during boot (e.g., root=/dev/sda1 quiet splash).
    ///
    /// Each string represents a multiple possible full command line string.
    pub kernel_cmdlines: Vec<String>,

    /// Raw byte content of the kernel image (e.g., vmlinuz).
    pub kernel: Vec<u8>,

    /// Raw byte content of the initial ramdisk (initrd or initramfs), used during early boot.
    pub initrd: Vec<u8>,
}

impl KernelArtifacts {
    pub async fn extract_cryptpilot_files(&self) -> Result<(FdeConfigBundle, Metadata)> {
        // First, create a tmp dir.
        let temp_dir = TempDir::new()?;
        let temp_dir_path = temp_dir.path();

        // Write the initrd content to a temporary file
        let initrd_path = temp_dir_path.join("initrd.img");
        tokio::fs::write(&initrd_path, &self.initrd)
            .await
            .context("Failed to write initrd content to a temporary dir")?;

        // Then, run lsinitrd --unpack to extract /etc/cryptpilot
        let _output = Command::new("lsinitrd")
            .arg("--unpack")
            .arg(&initrd_path)
            .arg(Path::new(CRYPTPILOT_CONFIG_DIR_DEFAULT).join("*"))
            .current_dir(temp_dir_path)
            .run()
            .await
            .context("Failed to unpack initrd")?;

        // Finally, load the config bundle from the extracted directory with:
        let config_dir = temp_dir_path.join(
            Path::new(CRYPTPILOT_CONFIG_DIR_DEFAULT)
                .strip_prefix("/")
                .unwrap_or(Path::new(CRYPTPILOT_CONFIG_DIR_DEFAULT)),
        );
        if !config_dir.exists() {
            anyhow::bail!("No cryptpilot config found in initrd");
        }

        let fde_config_bundle = FileSystemConfigSource::new(&config_dir)
            .get_config()
            .await
            .with_context(|| format!("Can not read config dir at {config_dir:?}"))?
            .strip_as_fde_config_bundle();

        let metadata = load_metadata_from_file(
            &temp_dir_path.join(
                Path::new(METADATA_PATH_IN_INITRD)
                    .strip_prefix("/")
                    .unwrap_or(Path::new(METADATA_PATH_IN_INITRD)),
            ),
        )
        .await
        .context("Can not load metadata from initrd")?;

        Ok((fde_config_bundle, metadata))
    }
}
