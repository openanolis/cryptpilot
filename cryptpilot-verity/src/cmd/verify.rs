use anyhow::Result;
use async_trait::async_trait;
use memmap2::Mmap;
use std::fs::File;
use tokio::fs;

use crate::cmd::{Command, DEFAULT_METADATA_FILE};

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
            metadata.clone()
        } else {
            self.options.data_dir.join(DEFAULT_METADATA_FILE)
        };

        tracing::info!("Reading metadata from: {:?}", metadata_path);

        // Read metadata file completely into memory for security
        // (Avoid TOCTOU attacks - we need immutable snapshot)
        let metadata_bytes = fs::read(&metadata_path).await?;

        // Calculate metadata hash (only from essential fields)
        let root_hash = crate::metadata::calculate_metadata_hash(&metadata_bytes)?;

        // Compare root hash with expected hash
        if root_hash != self.options.hash {
            anyhow::bail!(
                "Root hash mismatch. Expected: {}, Actual: {}",
                self.options.hash,
                root_hash
            );
        }

        tracing::info!("Root hash verification passed");

        // Parse metadata after hash verification
        let file_infos = crate::metadata::deserialize_metadata(&metadata_bytes)?;

        // Verify self-consistency of metadata entries (always required)
        for info in &file_infos {
            tracing::debug!("Verifying self-consistency for: {}", info.path);
            info.verify_self()?;
        }
        tracing::info!("Metadata self-consistency verification passed");

        // If metadata-only mode, skip file content verification
        if self.options.metadata_only {
            tracing::info!("Metadata-only mode: skipping file content verification");
        } else {
            // Verify each file using mmap (files can use mmap safely)
            for info in &file_infos {
                let file_path = self.options.data_dir.join(&info.path);
                tracing::debug!("Verifying file: {:?}", file_path);

                // Open and mmap the file
                let file = File::open(&file_path)
                    .map_err(|e| anyhow::anyhow!("Failed to open file {:?}: {}", file_path, e))?;

                // Safety: Opening regular file in read-only mode
                let mmap = unsafe { Mmap::map(&file)? };

                // Calculate fs-verity hash from mmap data
                let (calculated_descriptor, _calculated_merkle_tree) =
                    crate::metadata::calculate_fsverity_hash(&mmap);
                let calculated_descriptor_hash =
                    hex::encode(calculated_descriptor.to_descriptor_hash());

                // Verify descriptor hash
                if calculated_descriptor_hash != info.descriptor_hash {
                    anyhow::bail!(
                        "File descriptor hash mismatch for {}. Expected: {}, Actual: {}",
                        info.path,
                        info.descriptor_hash,
                        calculated_descriptor_hash
                    );
                }

                tracing::info!("File {} verified successfully", info.path);
            }
        }

        tracing::info!("All verifications passed");

        Ok(())
    }
}
