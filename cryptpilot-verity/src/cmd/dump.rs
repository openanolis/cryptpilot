use anyhow::Result;
use async_trait::async_trait;
use tokio::fs;

use crate::cmd::{Command, DEFAULT_METADATA_FILE};

pub struct DumpCommand {
    pub options: crate::cli::DumpOptions,
}

#[async_trait]
impl Command for DumpCommand {
    async fn run(&self) -> Result<()> {
        tracing::info!("Starting verity dump command");

        // Determine the metadata file path
        let metadata_path = if let Some(ref metadata) = self.options.metadata {
            metadata.clone()
        } else if let Some(ref data_dir) = self.options.data_dir {
            data_dir.join(DEFAULT_METADATA_FILE)
        } else {
            anyhow::bail!("Either data-dir or --metadata must be specified");
        };

        tracing::info!("Reading metadata from: {:?}", metadata_path);

        // Read metadata file into memory for security
        let metadata_bytes = fs::read(&metadata_path).await?;

        // Handle output based on flags
        if self.options.print_root_hash {
            // Calculate metadata hash (only from essential fields)
            let root_hash = crate::metadata::calculate_metadata_hash(&metadata_bytes)?;
            println!("{}", root_hash);
        } else if self.options.print_metadata {
            // Parse metadata
            let file_infos = crate::metadata::deserialize_metadata(&metadata_bytes)?;

            // Print metadata in human-readable format
            println!("Metadata contents:");
            println!("Total files: {}", file_infos.len());
            println!();
            for info in &file_infos {
                println!("File: {}", info.path);
                println!("  Descriptor Hash: {}", info.descriptor_hash);
                println!("  FsVerity Descriptor:");
                println!("    Version: {}", info.descriptor.version);
                println!(
                    "    Hash Algorithm: {} (1=SHA256, 2=SHA512)",
                    info.descriptor.hash_algorithm
                );
                println!("    Block Size: {} bytes", info.descriptor.block_size());
                println!("    Data Size: {} bytes", info.descriptor.data_size);
                println!("    Root Hash: {}", hex::encode(&info.descriptor.root_hash));
                if !info.descriptor.salt.is_empty() {
                    println!("    Salt: {}", hex::encode(&info.descriptor.salt));
                }
                println!(
                    "    Merkle Tree Level 1 Size: {} bytes",
                    info.merkle_tree.level1_as_bytes().len()
                );
                println!();
            }
        } else {
            anyhow::bail!("Either --print-root-hash or --print-metadata must be specified");
        };

        Ok(())
    }
}
