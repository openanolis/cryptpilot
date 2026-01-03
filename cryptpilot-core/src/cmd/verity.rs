use std::os::unix::process::CommandExt;

use anyhow::{Context, Result};
use async_trait::async_trait;

pub struct VerityCommand;

#[async_trait]
impl super::Command for VerityCommand {
    async fn run(&self) -> Result<()> {
        let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();

        args.remove(0);

        if let Some(idx) = args.iter().position(|arg| arg == "verity") {
            args.remove(idx);
        }

        Err(std::process::Command::new("cryptpilot-verity")
            .args(args)
            .exec())
        .context("Failed to execute `cryptpilot-verity`")?;

        Ok(())
    }
}
