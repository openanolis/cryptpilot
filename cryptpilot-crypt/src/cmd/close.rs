use anyhow::Result;
use async_trait::async_trait;

use crate::cli::CloseOptions;

pub struct CloseCommand {
    pub close_options: CloseOptions,
}

#[async_trait]
impl crate::cmd::Command for CloseCommand {
    async fn run(&self) -> Result<()> {
        for volume in &self.close_options.volume {
            tracing::info!("Close volume {volume} now");

            if !cryptpilot::fs::luks2::is_active(volume) {
                tracing::info!("The mapping for {} is not active, nothing to do", volume);
                continue;
            }

            tracing::info!("Removing mapping for {volume}");
            cryptpilot::fs::luks2::close(volume).await?;
            tracing::info!("The volume {volume} is closed now");
        }

        Ok(())
    }
}
