use anyhow::Result;
use async_trait::async_trait;
use log::info;

use crate::cli::CloseOptions;

pub struct CloseCommand {
    pub close_options: CloseOptions,
}

#[async_trait]
impl super::Command for CloseCommand {
    async fn run(&self) -> Result<()> {
        for volume in &self.close_options.volume {
            info!("Close volume {volume} now");

            if !crate::fs::luks2::is_active(&volume) {
                info!("The mapping for {} is not active, nothing to do", volume);
                continue;
            }

            info!("Removing mapping for {volume}");
            crate::fs::luks2::close(&volume).await?;
            info!("The volume {volume} is closed now");
        }

        Ok(())
    }
}
