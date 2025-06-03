pub mod cached;
pub mod cloud_init;
pub mod fs;

use anyhow::{anyhow, Context as _, Result};
use async_trait::async_trait;
use cached::CachedConfigSource;
use fs::FileSystemConfigSource;
use lazy_static::lazy_static;
use tokio::sync::{RwLock, RwLockReadGuard};

use super::{fde::FdeConfig, global::GlobalConfig, volume::VolumeConfig, ConfigBundle};

#[async_trait]
pub trait ConfigSource {
    fn source_debug_string(&self) -> String;

    async fn get_config(&self) -> Result<ConfigBundle>;

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>> {
        self.get_config()
            .await
            .map(|config| config.volumes)
            .with_context(|| format!("Failed to get volume configs for all volumes"))
    }

    async fn get_volume_config(&self, volume: &str) -> Result<VolumeConfig> {
        self.get_config() // TODO: move those functions to CachedConfigSource so that we can avoid clone all configs
            .await
            .map(|config| config.volumes)
            .and_then(|volume_configs| {
                let volume_config = volume_configs
                    .into_iter()
                    .find(|volume_config| volume_config.volume == volume)
                    .ok_or_else(|| anyhow!("Unknown volume name: {volume}"))?;

                Ok(volume_config)
            })
            .with_context(|| format!("Failed to get config for volume name: {}. Maybe forget to write config file for it?", volume))
    }

    async fn get_global_config(&self) -> Result<Option<GlobalConfig>> {
        self.get_config()
            .await
            .map(|config| config.global)
            .with_context(|| format!("Failed to get global config"))
    }

    async fn get_fde_config(&self) -> Result<Option<FdeConfig>> {
        self.get_config()
            .await
            .map(|config| config.fde)
            .with_context(|| format!("Failed to get FDE config"))
    }
}

lazy_static! {
    static ref CRYPTPILOT_CONFIG_SOURCE: RwLock<Box<dyn ConfigSource + Send + Sync>> =
        RwLock::new(Box::new(CachedConfigSource::new(
            FileSystemConfigSource::new_with_default_config_dir()
        )) as Box<dyn ConfigSource + Send + Sync>);
}

pub async fn set_config_source(config_source: impl ConfigSource + Send + Sync + 'static) {
    *(CRYPTPILOT_CONFIG_SOURCE.write().await) =
        Box::new(config_source) as Box<dyn ConfigSource + Send + Sync>;
}

pub async fn get_config_source() -> RwLockReadGuard<'static, Box<dyn ConfigSource + Send + Sync>> {
    CRYPTPILOT_CONFIG_SOURCE.read().await
}
