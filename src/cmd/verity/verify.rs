use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::cmd::Command;

const DEFAULT_METADATA_FILE: &str = "cryptpilot.metadata.json";

pub struct VerifyCommand {
    pub options: crate::cli::VerifyOptions,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileInfo {
    path: String,
    sha256: String,
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

        // Read metadata file
        let metadata_content = fs::read_to_string(&metadata_path).await?;
        let file_infos: Vec<FileInfo> = serde_json::from_str(&metadata_content)?;

        // Calculate overall directory root hash from metadata
        let mut hasher = Sha256::new();
        hasher.update(&metadata_content);
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
        for file_info in file_infos {
            let file_path = self.options.data_dir.join(&file_info.path);
            tracing::debug!("Verifying file: {:?}", file_path);

            // Read file content
            let content = fs::read(&file_path).await?;

            // Calculate file hash
            let hash = hex::encode(Sha256::digest(&content));

            // Compare with expected hash
            if hash != file_info.sha256 {
                anyhow::bail!(
                    "File hash mismatch for {}. Expected: {}, Actual: {}",
                    file_info.path,
                    file_info.sha256,
                    hash
                );
            }
        }

        tracing::info!("All file hash verifications passed");

        Ok(())
    }
}
