use anyhow::Result;
use async_trait::async_trait;

use crate::config::source::cloud_init::CLOUD_INIT_CONFIG_BUNDLE_HEADER;

pub struct DumpConfigCommand {}

#[async_trait]
impl super::Command for DumpConfigCommand {
    async fn run(&self) -> Result<()> {
        let config = crate::config::source::get_config_source()
            .await
            .get_config()
            .await?;

        println!(
            "{CLOUD_INIT_CONFIG_BUNDLE_HEADER}\n\n{}",
            toml::to_string(&config)?
        );
        Ok(())
    }
}
