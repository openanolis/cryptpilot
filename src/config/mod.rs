pub mod encrypt;
pub mod fde;
pub mod global;
pub mod source;
pub mod volume;

use fde::FdeConfig;
use global::GlobalConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigBundle {
    /// Global configuration. This is the same as the `/etc/cryptpilot/global.toml` file.
    pub global: Option<GlobalConfig>,

    /// Configuration related to full disk encryption (FDE). This is the same as the `/etc/cryptpilot/fde.toml` file.
    pub fde: Option<FdeConfig>,

    /// Configurations for data volumes. This is the same as the configs under `/etc/cryptpilot/volumes/` directory.
    #[serde(default = "Default::default")]
    pub volumes: Vec<volume::VolumeConfig>,
}
