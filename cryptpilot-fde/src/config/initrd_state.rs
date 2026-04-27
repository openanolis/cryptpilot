use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{FdeConfigBundle, FdeConfigSource};

pub const CRYPTPILOT_INITRD_STATE_PATH: &str = "/var/run/cryptpilot/initrd_state.toml";

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct InitrdState {
    pub fde_config_bundle: FdeConfigBundle,
}

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

pub struct InitrdStateConfigSource {}

impl Default for InitrdStateConfigSource {
    fn default() -> Self {
        Self::new()
    }
}

impl InitrdStateConfigSource {
    pub fn new() -> Self {
        Self {}
    }

    pub fn exist() -> bool {
        Path::new(CRYPTPILOT_INITRD_STATE_PATH).exists()
    }
}

#[async_trait]
impl FdeConfigSource for InitrdStateConfigSource {
    fn source_debug_string(&self) -> String {
        format!("initrd state: {CRYPTPILOT_INITRD_STATE_PATH}")
    }

    async fn get_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        Ok(InitrdState::load().await?.fde_config_bundle)
    }
}
