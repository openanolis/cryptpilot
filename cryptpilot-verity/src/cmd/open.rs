use anyhow::Result;
use async_trait::async_trait;
use fuser::MountOption;
use std::path::Path;
use tokio::fs;
use verity_fuse::file_verifier::{
    file_verity_info::FileVerityInfo, verity_verifier::VerityVerifier,
};
use verity_fuse::filesystem::VerityFS;

use crate::cmd::{is_mounted, Command, DEFAULT_METADATA_FILE, FUSE_FS_NAME, FUSE_SUBTYPE};

pub struct OpenCommand {
    pub options: crate::cli::OpenOptions,
}

#[async_trait]
impl Command for OpenCommand {
    async fn run(&self) -> Result<()> {
        tracing::info!("Starting open command");
        tracing::info!("Data directory: {:?}", self.options.data_dir);
        tracing::info!("Mount point: {:?}", self.options.mount_point);

        // Validate data directory exists
        if !self.options.data_dir.exists() {
            anyhow::bail!("Data directory does not exist: {:?}", self.options.data_dir);
        }
        if !self.options.data_dir.is_dir() {
            anyhow::bail!(
                "Data directory is not a directory: {:?}",
                self.options.data_dir
            );
        }

        // Validate mount point exists and is empty
        if !self.options.mount_point.exists() {
            anyhow::bail!("Mount point does not exist: {:?}", self.options.mount_point);
        }
        if !self.options.mount_point.is_dir() {
            anyhow::bail!(
                "Mount point is not a directory: {:?}",
                self.options.mount_point
            );
        }

        // Check if mount point is empty
        if is_mounted(&self.options.mount_point).await? {
            anyhow::bail!(
                "The mount point is already mounted with a verity-fuse filesystem: {:?}",
                self.options.mount_point
            );
        }

        // Determine metadata file path
        let metadata_path = if let Some(ref metadata) = self.options.metadata {
            metadata.clone()
        } else {
            self.options.data_dir.join(DEFAULT_METADATA_FILE)
        };

        tracing::info!("Reading metadata from: {:?}", metadata_path);

        // Read and verify metadata
        let metadata_bytes = fs::read(&metadata_path).await?;

        // Calculate metadata hash
        let root_hash = crate::metadata::calculate_metadata_hash(&metadata_bytes)?;

        // Verify root hash matches expected
        if root_hash != self.options.hash {
            anyhow::bail!(
                "Metadata hash mismatch. Expected: {}, Actual: {}",
                self.options.hash,
                root_hash
            );
        }

        tracing::info!("Metadata hash verification passed");

        // Parse metadata
        let file_infos = crate::metadata::deserialize_metadata(&metadata_bytes)?;
        tracing::info!("Metadata contains {} files", file_infos.len());

        // Verify metadata integrity for each file
        tracing::info!("Verifying metadata integrity...");
        for info in &file_infos {
            info.verify_self().map_err(|e| {
                anyhow::anyhow!("Metadata verification failed for {}: {}", info.path, e)
            })?;
        }
        tracing::info!("Metadata integrity verification passed");

        // Mount using verity-fuse with real verification
        self.mount_verity_fuse(
            &self.options.data_dir,
            &self.options.mount_point,
            file_infos,
        )
        .await?;

        Ok(())
    }
}

impl OpenCommand {
    async fn mount_verity_fuse(
        &self,
        source: &Path,
        mount_point: &Path,
        file_infos: Vec<FileVerityInfo>,
    ) -> Result<()> {
        tracing::info!(
            "Initializing verity-fuse filesystem with {} files",
            file_infos.len()
        );

        // Create VerityVerifier from metadata
        let verifier = VerityVerifier::new(file_infos)?;

        // Create VerityFS instance with real verifier
        let fs = VerityFS::new(source, verifier)?;

        // Prepare mount options
        let options = vec![
            MountOption::RO,
            MountOption::AutoUnmount,
            MountOption::AllowOther,
            MountOption::FSName(FUSE_FS_NAME.to_string()),
            MountOption::Subtype(FUSE_SUBTYPE.to_string()),
        ];

        tracing::info!("Mounting verity-fuse fs with verity enabled");
        // Mount in foreground - this will block
        fuser::mount2(fs, mount_point, &options)?;
        Ok(())
    }
}
