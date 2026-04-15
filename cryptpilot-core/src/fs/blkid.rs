use std::path::Path;

use anyhow::{bail, Context as _, Result};
use tokio::process::Command;

use crate::fs::cmd::CheckCommandOutput;

/// Result of probing a block device with `blkid -p`.
#[derive(Debug, Clone)]
pub enum BlkidProbeResult {
    /// No filesystem signatures detected on the device.
    NoSignatures,
    /// A known filesystem or partition type was detected.
    KnownSignature {
        /// Filesystem type from `TYPE="..."`, if present.
        fs_type: Option<String>,
        /// Partition table type from `PTTYPE="..."`, if present.
        pt_type: Option<String>,
    },
}

impl BlkidProbeResult {
    /// Returns `true` if no signature was detected.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::NoSignatures)
    }

    /// Returns `true` if the device contains dm-snapshot COW metadata.
    pub fn is_dm_snapshot_cow(&self) -> bool {
        match self {
            Self::KnownSignature {
                fs_type: Some(fs_type),
                ..
            } => fs_type.contains("DM_snapshot_cow"),
            _ => false,
        }
    }
}

/// Probe a block device using `blkid -p` and parse the output.
///
/// Exit codes:
/// - `0`: signatures detected → `KnownSignature`
/// - `2`: no signatures detected → `NoSignatures`
/// - Other: returns an error
///
/// Note: `PTTYPE="atari"` is a known false positive on blank devices
/// (see <https://bugs.launchpad.net/ubuntu/+source/util-linux/+bug/2015355>)
/// and is treated as `NoSignatures`.
pub async fn probe_device(device_path: &Path) -> Result<BlkidProbeResult> {
    Command::new("blkid")
        .arg("-p")
        .arg(device_path)
        .env("LC_ALL", "C")
        .run_with_status_checker(|code, stdout, _stderr| {
            match code {
                0 => {
                    let output = String::from_utf8_lossy(&stdout);
                    let trimmed = output.trim().to_string();

                    // Parse TYPE="..."
                    let fs_type = parse_blkid_field(&trimmed, "TYPE");
                    // Parse PTTYPE="..."
                    let pt_type = parse_blkid_field(&trimmed, "PTTYPE");

                    // Treat atari partition table as no signature (known false positive)
                    // See: https://bugs.launchpad.net/ubuntu/+source/util-linux/+bug/2015355
                    if pt_type.as_deref() == Some("atari") && fs_type.is_none() {
                        tracing::info!(
                            "blkid probe on {device_path:?}: PTTYPE=\"atari\" (known false positive), treating as no signature"
                        );
                        return Ok(BlkidProbeResult::NoSignatures);
                    }

                    tracing::info!(
                        "blkid probe on {device_path:?}: fs_type={fs_type:?}, pt_type={pt_type:?}"
                    );

                    Ok(BlkidProbeResult::KnownSignature { fs_type, pt_type })
                }
                2 => {
                    // blkid exit code 2 = no signatures found
                    Ok(BlkidProbeResult::NoSignatures)
                }
                _ => {
                    let stdout = String::from_utf8_lossy(&stdout);
                    let stderr = String::from_utf8_lossy(&_stderr);
                    bail!(
                        "blkid probe failed with exit code {}: stdout='{}', stderr='{}'",
                        code,
                        stdout.trim(),
                        stderr.trim()
                    )
                }
            }
        })
        .await
        .context("Failed to execute blkid probe")
}

/// Extract a field value from blkid output.
///
/// blkid outputs key-value pairs like: `TYPE="ext4" PTTYPE="dos"`
fn parse_blkid_field(output: &str, key: &str) -> Option<String> {
    let pattern = format!("{}=\"", key);
    let start = output.find(&pattern)? + pattern.len();
    let rest = &output[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
