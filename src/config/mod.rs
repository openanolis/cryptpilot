pub mod encrypt;
pub mod fde;
pub mod global;
pub mod source;
pub mod volume;

use anyhow::Result;
use async_trait::async_trait;
use fde::FdeConfig;
use global::GlobalConfig;
use serde::{Deserialize, Serialize};
use source::{cloud_init::FdeConfigBundle, ConfigSource};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigBundle {
    /// Global configuration. This is the same as the `/etc/cryptpilot/global.toml` file.
    pub global: Option<GlobalConfig>,

    /// Configuration related to full disk encryption (FDE). This is the same as the `/etc/cryptpilot/fde.toml` file.
    pub fde: Option<FdeConfig>,

    /// Configurations for data volumes. This is the same as the configs under `/etc/cryptpilot/volumes/` directory.
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Default::default")]
    pub volumes: Vec<volume::VolumeConfig>,
}

#[async_trait]
impl ConfigSource for ConfigBundle {
    fn source_debug_string(&self) -> String {
        "in-memory config bundle".to_owned()
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        Ok(self.clone())
    }
}

impl ConfigBundle {
    pub fn strip_as_fde_config_bundle(self) -> FdeConfigBundle {
        FdeConfigBundle {
            global: self.global,
            fde: self.fde,
        }
    }
}
