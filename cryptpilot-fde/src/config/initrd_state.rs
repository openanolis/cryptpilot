use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use crate::cmd::boot_service::initrd_state::{InitrdState, CRYPTPILOT_INITRD_STATE_PATH};

use super::{FdeConfigBundle, FdeConfigSource};

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
