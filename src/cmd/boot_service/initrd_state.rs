use std::path::Path;

use crate::config::{source::ConfigSource, ConfigBundle};

use anyhow::{Context, Result};
use async_trait::async_trait;
use log::info;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct InitrdState {
    pub config: ConfigBundle,
}

pub const CRYPTPILOT_INITRD_STATE_PATH: &'static str = "/var/run/cryptpilot/initrd_state.toml";

pub async fn serialize_initrd_state(initrd_state: &InitrdState) -> Result<()> {
    let str: String = toml::to_string_pretty(&initrd_state).unwrap();
    let path = Path::new(CRYPTPILOT_INITRD_STATE_PATH);
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    tokio::fs::write(CRYPTPILOT_INITRD_STATE_PATH, str).await?;
    info!("Successfully wrote initrd state to {CRYPTPILOT_INITRD_STATE_PATH}");
    Ok(())
}

pub async fn deserialize_initrd_state() -> Result<InitrdState> {
    tokio::fs::read_to_string(CRYPTPILOT_INITRD_STATE_PATH)
        .await
        .map_err(anyhow::Error::from)
        .and_then(|str| toml::from_str(&str).map_err(anyhow::Error::from))
        .with_context(|| format!("Failed to read initrd state from {CRYPTPILOT_INITRD_STATE_PATH}"))
}

pub struct InitrdStateConfigSource {}

impl InitrdStateConfigSource {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ConfigSource for InitrdStateConfigSource {
    fn source_debug_string(&self) -> String {
        format!("initrd state: {CRYPTPILOT_INITRD_STATE_PATH}")
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        Ok(deserialize_initrd_state().await?.config)
    }
}
