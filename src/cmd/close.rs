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
        let volume = self.close_options.volume.to_owned();

        if !crate::fs::luks2::is_active(&volume) {
            info!("The mapping for {} is not active, nothing to do", volume);
            return Ok(());
        }

        info!("Removing mapping for {volume}");
        crate::fs::luks2::close(&volume).await?;
        info!("The mapping is removed now");

        Ok(())
    }
}
