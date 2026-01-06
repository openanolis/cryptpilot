use anyhow::{Context, Result};
use async_trait::async_trait;

mod close;
mod dump;
mod format;
mod open;
mod verify;

pub const FUSE_FS_NAME: &str = "verity-fuse";
pub const FUSE_SUBTYPE: &str = "verity-fuse";

pub const DEFAULT_METADATA_FILE: &str = "cryptpilot-verity.metadata.fb";

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::VeritySubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::VeritySubcommand::Format(format_options) => {
                Box::new(format::FormatCommand {
                    options: format_options,
                })
            }
            crate::cli::VeritySubcommand::Verify(verify_options) => {
                Box::new(verify::VerifyCommand {
                    options: verify_options,
                })
            }
            crate::cli::VeritySubcommand::Dump(dump_options) => Box::new(dump::DumpCommand {
                options: dump_options,
            }),
            crate::cli::VeritySubcommand::Open(open_options) => Box::new(open::OpenCommand {
                options: open_options,
            }),
            crate::cli::VeritySubcommand::Close(close_options) => Box::new(close::CloseCommand {
                options: close_options,
            }),
        }
    }
}

pub async fn is_mounted(mount_point: &std::path::Path) -> Result<bool> {
    async {
        // Get canonical path for comparison
        let canonical_mount_point = tokio::fs::canonicalize(mount_point).await?;

        // Check all mount points
        let mountinfo = mountinfo2::MountInfo::new()?;

        for mount_point in mountinfo.mounting_points {
            // Check if mount point matches
            if mount_point.path == canonical_mount_point {
                // Check if filesystem type is fuse (verity-fuse uses FUSE)

                if let mountinfo2::FsType::Other(fstype) = &mount_point.fstype {
                    if fstype == "fuse" || fstype == &format!("fuse.{FUSE_SUBTYPE}") {
                        if mount_point.what == FUSE_FS_NAME {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok::<_, anyhow::Error>(false)
    }
    .await
    .context("Failed to check if mounted")
}
