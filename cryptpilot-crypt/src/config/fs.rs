use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::config::VolumeConfig;

use super::VolumeConfigSource;

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
impl VolumeConfigSource for FileSystemConfigSource {
    fn source_debug_string(&self) -> String {
        format!("filesystem(volume): {:?}", self.config_dir)
    }

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        self.load_volume_configs().await
    }
}
