use anyhow::Result;
use async_trait::async_trait;
use async_walkdir::WalkDir;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::cmd::Command;

const DEFAULT_METADATA_FILE: &str = "cryptpilot.metadata.json";

pub struct FormatCommand {
    pub options: crate::cli::FormatOptions,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileInfo {
    path: String,
    sha256: String,
}

#[async_trait]
impl Command for FormatCommand {
    async fn run(&self) -> Result<()> {
        tracing::info!("Starting verity format command");
        tracing::info!("Data directory: {:?}", self.options.data_dir);

        // Collect all file paths
        let mut files = Vec::new();
        self.collect_files(&self.options.data_dir, &mut files)
            .await?;

        tracing::info!("Found {} files in data directory", files.len());

        // Sort file paths to ensure deterministic output
        files.sort();

        // Calculate hash for each file
        let mut file_infos = Vec::new();
        for file_path in files {
            tracing::debug!("Processing file: {:?}", file_path);
            let content = fs::read(&file_path).await?;
            let hash = hex::encode(Sha256::digest(&content));

            let relative_path = file_path
                .strip_prefix(&self.options.data_dir)?
                .to_path_buf();
            let path_str = relative_path.to_string_lossy().to_string();

            file_infos.push(FileInfo {
                path: path_str,
                sha256: hash,
            });
        }

        // Generate JSON metadata
        let json = serde_json::to_string_pretty(&file_infos)?;
        tracing::debug!("Generated metadata JSON with {} entries", file_infos.len());

        // Determine the actual metadata file path
        let metadata_path = if let Some(ref metadata) = self.options.metadata {
            if metadata.is_absolute() {
                metadata.clone()
            } else {
                self.options.data_dir.join(metadata)
            }
        } else {
            self.options.data_dir.join(DEFAULT_METADATA_FILE)
        };

        tracing::info!("Writing metadata to: {:?}", metadata_path);

        // Write JSON metadata to file
        fs::write(&metadata_path, &json).await?;

        // Calculate overall directory root hash
        let mut hasher = Sha256::new();
        hasher.update(&json);
        let root_hash = hex::encode(hasher.finalize());

        tracing::info!("Root hash calculated: {}", root_hash);

        // Write root hash to specified output or stdout
        if self.options.hash_output.as_os_str() == "-" {
            println!("{}", root_hash);
        } else {
            tracing::info!("Writing root hash to: {:?}", self.options.hash_output);
            fs::write(&self.options.hash_output, &root_hash).await?;
        }

        Ok(())
    }
}

impl FormatCommand {
    async fn collect_files(&self, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = WalkDir::new(dir);

        while let Some(Ok(entry)) = entries.next().await {
            if entry.path().file_name() == Some(std::ffi::OsStr::new(DEFAULT_METADATA_FILE)) {
                continue;
            }

            if entry.file_type().await.map_or(false, |ft| ft.is_file()) {
                files.push(entry.path());
            }
        }

        Ok(())
    }
}
