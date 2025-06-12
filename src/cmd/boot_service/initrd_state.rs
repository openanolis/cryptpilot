use std::path::Path;

use crate::config::source::cloud_init::FdeConfigBundle;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct InitrdState {
    pub fde_config_bundle: FdeConfigBundle,
}

pub const CRYPTPILOT_INITRD_STATE_PATH: &'static str = "/var/run/cryptpilot/initrd_state.toml";

impl InitrdState {
    pub async fn save(&self) -> Result<()> {
        let str: String = toml::to_string_pretty(self).unwrap();
        let path = Path::new(CRYPTPILOT_INITRD_STATE_PATH);
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        tokio::fs::write(CRYPTPILOT_INITRD_STATE_PATH, str).await?;
        tracing::info!("Successfully wrote initrd state to {CRYPTPILOT_INITRD_STATE_PATH}");
        Ok(())
    }
    pub async fn load() -> Result<InitrdState> {
        tokio::fs::read_to_string(CRYPTPILOT_INITRD_STATE_PATH)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|str| toml::from_str(&str).map_err(anyhow::Error::from))
            .with_context(|| {
                format!("Failed to read initrd state from {CRYPTPILOT_INITRD_STATE_PATH}")
            })
    }
}
