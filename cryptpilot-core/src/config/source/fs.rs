use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;

use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::fde::FdeConfig;
use crate::config::ConfigBundle;

use super::super::global::GlobalConfig;
use super::super::volume::VolumeConfig;
use super::ConfigSource;

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

    async fn load_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        let mut volume_configs = Vec::new();
        let config_dir = self.config_dir.join("volumes");

        tracing::debug!("Loading volume configs from: {config_dir:?}");
        if !config_dir.exists() {
            tracing::debug!("Volume configs directory not found, skip: {config_dir:?}");
            return Ok(vec![]);
        }

        let mut volume_names = HashSet::<String>::new();

        let mut entries = tokio::fs::read_dir(config_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() && path.extension().map_or(false, |ext| ext == "toml") {
                let volume_config = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(Into::into)
                    .and_then(|content| {
                        toml::from_str::<VolumeConfig>(&content)
                            .context("Failed to parse content as TOML")
                    })
                    .and_then(|volume_config| {
                        if volume_names.contains(&volume_config.volume) {
                            bail!(
                                "Volume `{}` is already defined in other volume config files. Please checking your volume config files.",
                                volume_config.volume
                            )
                        }
                        volume_names.insert(volume_config.volume.to_owned());
                        Ok(volume_config)
                    })
                    .with_context(|| format!("Failed to loading volume config file: {}", path.display()))?;

                volume_configs.push(volume_config);
            }
        }

        volume_configs.sort_by(|a, b| a.volume.cmp(&b.volume));

        Ok(volume_configs)
    }
}

#[async_trait]
impl ConfigSource for FileSystemConfigSource {
    fn source_debug_string(&self) -> String {
        format!("filesystem: {:?}", self.config_dir)
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        Ok(ConfigBundle {
            global: self.load_global_config().await?,
            fde: self.load_fde_config().await?,
            volumes: self.load_volume_configs().await?,
        })
    }
}
