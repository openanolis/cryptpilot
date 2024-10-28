use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

use anyhow::{bail, Context as _, Result};
use lazy_static::lazy_static;
use log::debug;
use serde::{Deserialize, Serialize};

const CRYPTPILOT_CONFIG_DIR_DEFAULT: &'static str = "/etc/cryptpilot";

lazy_static! {
    static ref CRYPTPILOT_CONFIG_DIR: RwLock<PathBuf> =
        RwLock::new(CRYPTPILOT_CONFIG_DIR_DEFAULT.into());
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct VolumeConfig {
    /// The name of resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
    pub volume: String,

    /// The identifier of the underlying encrypted device.
    pub dev: String,

    /// The key provider specific options
    pub key_provider: KeyProviderOptions,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KeyProviderOptions {
    #[cfg(feature = "provider-temp")]
    Temp(crate::provider::temp::TempOptions),
    #[cfg(feature = "provider-kms")]
    Kms(crate::provider::kms::KmsOptions),
    #[cfg(feature = "provider-kbs")]
    Kbs(crate::provider::kbs::KbsOptions),
    #[cfg(feature = "provider-tpm2")]
    Tpm2(crate::provider::tpm2::Tpm2Options),
}

pub fn set_config_dir(config_dir: impl AsRef<Path>) -> Result<()> {
    *(CRYPTPILOT_CONFIG_DIR
        .write()
        .or_else(|e| bail!("Failed to set cryptpilot configs dir: {e}"))?) =
        PathBuf::from(config_dir.as_ref());
    Ok(())
}

pub fn get_config_dir() -> Result<PathBuf> {
    Ok(CRYPTPILOT_CONFIG_DIR
        .read()
        .or_else(|e| bail!("Failed to get cryptpilot configs dir: {e}"))?
        .clone())
}

pub fn load_volume_configs() -> Result<Vec<VolumeConfig>> {
    let mut configs = Vec::new();
    let config_dir = get_config_dir()?.join("volumes");

    debug!("Loading volume configs from: {config_dir:?}");
    if !config_dir.exists() {
        bail!("Directory not found: {}", config_dir.display());
    }

    for entry in std::fs::read_dir(config_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "conf") {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file: {}", path.display()))?;

            let config: VolumeConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse TOML from file: {}", path.display()))?;

            configs.push(config);
        }
    }

    Ok(configs)
}

#[cfg(test)]
pub mod tests {
    use anyhow::Result;

    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_deserialize() -> Result<()> {
        let raw = r#"
        dev = "/dev/nvme1n1p1"
        volume = "data"

        [key_provider.temp]
        "#;

        let config: VolumeConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            VolumeConfig {
                volume: "data".into(),
                dev: "/dev/nvme1n1p1".into(),
                key_provider: KeyProviderOptions::Temp(crate::provider::temp::TempOptions {}),
            }
        );
        Ok(())
    }
}
