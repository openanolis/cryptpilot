use std::path::Path;

use anyhow::{Context as _, Result};
use tokio::process::Command;

use crate::fs::cmd::CheckCommandOutput as _;

/// Partition table type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartitionTableType {
    Mbr,
    Gpt,
}

impl PartitionTableType {
    /// Detect partition table type by using fdisk -l command
    pub async fn detect_partition_table_type(disk_dev: &Path) -> Result<PartitionTableType> {
        // Run fdisk -l command to get disk information
        let stdout = Command::new("fdisk")
            .args(["-l", &disk_dev.to_string_lossy()])
            .run()
            .await
            .with_context(|| format!("Failed to execute fdisk -l for {:?}", disk_dev))?;

        let stdout_str = String::from_utf8_lossy(&stdout);

        // Parse the output to determine partition table type
        // Look for lines that indicate the partition table type
        for line in stdout_str.lines() {
            if line.contains("Disklabel type: gpt") {
                return Ok(PartitionTableType::Gpt);
            } else if line.contains("Disklabel type: dos") {
                return Ok(PartitionTableType::Mbr);
            }
        }

        // If we can't determine the type from the output, default to GPT
        tracing::debug!(
            "Cannot determine partition table type for {:?} from fdisk output, defaulting to GPT",
            disk_dev
        );
        Ok(PartitionTableType::Gpt)
    }
}
