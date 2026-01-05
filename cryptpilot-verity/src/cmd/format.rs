use anyhow::Result;
use async_trait::async_trait;
use async_walkdir::WalkDir;
use futures::StreamExt;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::cmd::Command;

const DEFAULT_METADATA_FILE: &str = "cryptpilot.metadata.fb";

pub struct FormatCommand {
    pub options: crate::cli::FormatOptions,
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

        // Calculate fsverity hash for each file
        let mut file_infos = Vec::new();
        for file_path in files {
            tracing::debug!("Processing file: {:?}", file_path);
            let content = fs::read(&file_path).await?;

            // Calculate fs-verity hash
            let (descriptor, merkle_tree) = crate::metadata::calculate_fsverity_hash(&content);

            let relative_path = file_path
                .strip_prefix(&self.options.data_dir)?
                .to_path_buf();
            let path_str = relative_path.to_string_lossy().to_string();

            let descriptor_hash = hex::encode(descriptor.to_descriptor_hash());

            let info = crate::metadata::FileVerityInfo {
                path: path_str,
                descriptor,
                merkle_tree_level1: merkle_tree.level1_as_bytes(),
                descriptor_hash: descriptor_hash.clone(),
            };

            tracing::debug!(
                "File: {:?}, descriptor_hash: {}",
                file_path,
                descriptor_hash
            );
            file_infos.push(info);
        }

        // Serialize to FlatBuffers format
        let fb_data = crate::metadata::serialize_metadata(&file_infos)?;
        tracing::debug!(
            "Generated FlatBuffers metadata with {} entries",
            file_infos.len()
        );

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

        // Write FlatBuffers metadata to file
        fs::write(&metadata_path, &fb_data).await?;

        // Calculate metadata hash (only from essential fields)
        let root_hash = crate::metadata::calculate_metadata_hash(&fb_data)?;

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
