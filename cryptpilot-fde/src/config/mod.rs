pub mod cached;
pub mod cloud_init;
pub mod fde;
pub mod fs;
pub mod global;
pub mod initrd_state;

use anyhow::Result;
use async_trait::async_trait;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::sync::{RwLock, RwLockReadGuard};

use cached::CachedFdeConfigSource;
pub use fde::*;
use fs::FileSystemConfigSource;
pub use global::*;

/// FDE configuration bundle that combines global and FDE-specific configurations.
/// This structure is used when loading FDE configuration from various sources.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct FdeConfigBundle {
    /// Global configuration. This is the same as the `/etc/cryptpilot/global.toml` file.
    pub global: Option<GlobalConfig>,

    /// Configuration related to full disk encryption (FDE). This is the same as the `/etc/cryptpilot/fde.toml` file.
    pub fde: Option<FdeConfig>,
}

impl FdeConfigBundle {
    pub fn gen_hash_content(&self) -> Result<String> {
        Ok(toml::to_string(&self)?)
    }

    pub fn gen_hash_content_pretty(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self)?)
    }

    pub fn gen_hash_hex(&self) -> Result<String> {
        let content_to_hash = self.gen_hash_content()?;
        let hash = sha2::Sha384::new()
            .chain_update(content_to_hash)
            .finalize()
            .to_vec();
        let hash_hex = hex::encode(hash);

        Ok(hash_hex)
    }
}

#[async_trait]
pub trait FdeConfigSource {
    fn source_debug_string(&self) -> String;

    async fn get_fde_config_bundle(&self) -> Result<FdeConfigBundle>;

    async fn get_global_config(&self) -> Result<Option<GlobalConfig>> {
        Ok(self.get_fde_config_bundle().await?.global)
    }

    async fn get_fde_config(&self) -> Result<Option<FdeConfig>> {
        Ok(self.get_fde_config_bundle().await?.fde)
    }
}

lazy_static! {
    static ref CRYPTPILOT_FDE_CONFIG_SOURCE: RwLock<Box<dyn FdeConfigSource + Send + Sync>> =
        RwLock::new(Box::new(CachedFdeConfigSource::new(
            FileSystemConfigSource::new_with_default_config_dir()
        )) as Box<dyn FdeConfigSource + Send + Sync>);
}

pub async fn set_fde_config_source(config_source: impl FdeConfigSource + Send + Sync + 'static) {
    *(CRYPTPILOT_FDE_CONFIG_SOURCE.write().await) =
        Box::new(config_source) as Box<dyn FdeConfigSource + Send + Sync>;
}

pub async fn get_fde_config_source(
) -> RwLockReadGuard<'static, Box<dyn FdeConfigSource + Send + Sync>> {
    CRYPTPILOT_FDE_CONFIG_SOURCE.read().await
}
