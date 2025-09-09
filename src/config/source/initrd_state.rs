use std::path::Path;

use crate::{
    cmd::boot_service::initrd_state::{InitrdState, CRYPTPILOT_INITRD_STATE_PATH},
    config::{source::ConfigSource, ConfigBundle},
};
use anyhow::Result;
use async_trait::async_trait;

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
impl ConfigSource for InitrdStateConfigSource {
    fn source_debug_string(&self) -> String {
        format!("initrd state: {CRYPTPILOT_INITRD_STATE_PATH}")
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        Ok(InitrdState::load()
            .await?
            .fde_config_bundle
            .flatten_to_config_bundle())
    }
}
