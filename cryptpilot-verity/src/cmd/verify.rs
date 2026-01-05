use anyhow::Result;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::cmd::Command;

const DEFAULT_METADATA_FILE: &str = "cryptpilot.metadata.fb";

pub struct VerifyCommand {
    pub options: crate::cli::VerifyOptions,
}

#[async_trait]
impl Command for VerifyCommand {
    async fn run(&self) -> Result<()> {
        tracing::info!("Starting verity verify command");
        tracing::info!("Data directory: {:?}", self.options.data_dir);
        tracing::info!("Expected root hash: {}", self.options.hash);

        // Determine the metadata file path
        let metadata_path = if let Some(ref metadata) = self.options.metadata {
            if metadata.is_absolute() {
                metadata.clone()
            } else {
                self.options.data_dir.join(metadata)
            }
        } else {
            self.options.data_dir.join(DEFAULT_METADATA_FILE)
        };

        tracing::info!("Reading metadata from: {:?}", metadata_path);

        // Read metadata file as binary
        let metadata_bytes = fs::read(&metadata_path).await?;
        let file_infos = crate::metadata::deserialize_metadata(&metadata_bytes)?;

        // Calculate overall directory root hash from metadata
        let mut hasher = Sha256::new();
        hasher.update(&metadata_bytes);
        let root_hash = hex::encode(hasher.finalize());

        // Compare root hash with expected hash
        if root_hash != self.options.hash {
            anyhow::bail!(
                "Root hash mismatch. Expected: {}, Actual: {}",
                self.options.hash,
                root_hash
            );
        }

        tracing::info!("Root hash verification passed");

        // Verify each file
        for info in file_infos {
            let file_path = self.options.data_dir.join(&info.path);
            tracing::debug!("Verifying file: {:?}", file_path);

            // Read file content
            let content = fs::read(&file_path).await?;

            // Calculate fs-verity hash
            let calculated_info = crate::metadata::calculate_fsverity_hash(&content)?;

            // Compare with expected descriptor hash
            if calculated_info.descriptor_hash != info.descriptor_hash {
                anyhow::bail!(
                    "File descriptor hash mismatch for {}. Expected: {}, Actual: {}",
                    info.path,
                    info.descriptor_hash,
                    calculated_info.descriptor_hash
                );
            }
            
            tracing::debug!("File {} verified successfully", info.path);
        }

        tracing::info!("All file hash verifications passed");

        Ok(())
    }
}
