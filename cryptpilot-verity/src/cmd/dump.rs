use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::cmd::Command;

const DEFAULT_METADATA_FILE: &str = "cryptpilot.metadata.json";

pub struct DumpCommand {
    pub options: crate::cli::DumpOptions,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileInfo {
    path: String,
    sha256: String,
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
            anyhow::bail!("Either --metadata or --data-dir must be specified");
        };

        tracing::info!("Reading metadata from: {:?}", metadata_path);

        // Read metadata file
        let metadata_content = fs::read_to_string(&metadata_path).await?;
        let file_infos: Vec<FileInfo> = serde_json::from_str(&metadata_content)?;

        // Handle output based on flags
        if self.options.print_root_hash {
            // Calculate and print root hash
            let mut hasher = Sha256::new();
            hasher.update(&metadata_content);
            let root_hash = hex::encode(hasher.finalize());
            println!("{}", root_hash);
        } else if self.options.print_metadata {
            // Print full metadata JSON
            let metadata_content = serde_json::to_string_pretty(&file_infos)?;
            println!("{}", metadata_content);
        } else {
            anyhow::bail!("Either --print-root-hash or --print-metadata must be specified");
        };

        Ok(())
    }
}
