use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;

use crate::config::{FdeConfig, GlobalConfig};

use super::{FdeConfigBundle, FdeConfigSource};

pub const CRYPTPILOT_CONFIG_DIR_DEFAULT: &str = "/etc/cryptpilot";

pub struct FileSystemConfigSource {
    config_dir: PathBuf,
}

impl FileSystemConfigSource {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    pub fn new_with_default_config_dir() -> Self {
        Self::new(PathBuf::from(CRYPTPILOT_CONFIG_DIR_DEFAULT))
    }

    async fn load_global_config(&self) -> Result<Option<GlobalConfig>> {
        let config_path = self.config_dir.join("global.toml");

        tracing::debug!("Loading global config from: {config_path:?}");
        if !config_path.exists() {
            tracing::debug!("global config not found, skip: {config_path:?}");
            return Ok(None);
        }

        let global_config = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|content| {
                toml::from_str::<GlobalConfig>(&content).context("Failed to parse content as TOML")
            })
            .with_context(|| format!("Failed to load global config from: {config_path:?}"))?;

        Ok(Some(global_config))
    }

    async fn load_fde_config(&self) -> Result<Option<FdeConfig>> {
        let config_path = self.config_dir.join("fde.toml");

        if !config_path.exists() {
            tracing::debug!("FDE config not found, skip: {config_path:?}");
            return Ok(None);
        }
        tracing::debug!("Loading FDE config from: {config_path:?}");

        let fde_config = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|content| {
                toml::from_str::<FdeConfig>(&content).context("Failed to parse content as TOML")
            })
            .with_context(|| format!("Failed to load FDE config from: {config_path:?}"))?;

        Ok(Some(fde_config))
    }
}

#[async_trait]
impl FdeConfigSource for FileSystemConfigSource {
    fn source_debug_string(&self) -> String {
        format!(
            "filesystem: global at {:?}, fde at {:?}",
            self.config_dir.join("global.toml"),
            self.config_dir.join("fde.toml")
        )
    }

    async fn get_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        let global = self.load_global_config().await?;
        let fde = self.load_fde_config().await?;
        Ok(FdeConfigBundle { global, fde })
    }
}
