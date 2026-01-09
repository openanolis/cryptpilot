pub mod cached;
pub mod fs;
pub mod volume;

// Alias for backward compatibility with tests
#[allow(unused_imports)]
pub mod source {
    pub use super::{set_volume_config_source, VolumeConfigSource};
}

use anyhow::{anyhow, Context as _, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use tokio::sync::{RwLock, RwLockReadGuard};

use cached::CachedVolumeConfigSource;
use fs::FileSystemConfigSource;
pub use volume::*;

#[async_trait]
pub trait VolumeConfigSource {
    fn source_debug_string(&self) -> String;

    async fn get_volume_configs(&self) -> Result<Vec<VolumeConfig>>;

    async fn get_volume_config(&self, volume: &str) -> Result<VolumeConfig> {
        self.get_volume_configs()
            .await
            .and_then(|volume_configs| {
                let volume_config = volume_configs
                    .into_iter()
                    .find(|volume_config| volume_config.volume == volume)
                    .ok_or_else(|| anyhow!("Unknown volume name: {volume}"))?;
                Ok(volume_config)
            })
            .with_context(|| {
                format!(
                    "Failed to get config for volume name: {}. Maybe forget to write config file for it?",
                    volume
                )
            })
    }
}

lazy_static! {
    static ref CRYPTPILOT_VOLUME_CONFIG_SOURCE: RwLock<Box<dyn VolumeConfigSource + Send + Sync>> =
        RwLock::new(Box::new(CachedVolumeConfigSource::new(
            FileSystemConfigSource::new_with_default_config_dir()
        )) as Box<dyn VolumeConfigSource + Send + Sync>);
}

pub async fn set_volume_config_source(
    config_source: impl VolumeConfigSource + Send + Sync + 'static,
) {
    *(CRYPTPILOT_VOLUME_CONFIG_SOURCE.write().await) =
        Box::new(config_source) as Box<dyn VolumeConfigSource + Send + Sync>;
}

pub async fn get_volume_config_source(
) -> RwLockReadGuard<'static, Box<dyn VolumeConfigSource + Send + Sync>> {
    CRYPTPILOT_VOLUME_CONFIG_SOURCE.read().await
}
