use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context as _, Result};
use lazy_static::lazy_static;
use log::debug;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const CRYPTPILOT_CONFIG_DIR_DEFAULT: &'static str = "/etc/cryptpilot";

lazy_static! {
    static ref CRYPTPILOT_CONFIG_DIR: RwLock<PathBuf> =
        RwLock::new(CRYPTPILOT_CONFIG_DIR_DEFAULT.into());
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct VolumeConfig {
    /// The name of resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
    pub volume: String,

    /// The identifier of the underlying encrypted device.
    pub dev: String,

    /// The key provider specific options
    pub key_provider: KeyProviderOptions,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
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

pub async fn set_config_dir(config_dir: impl AsRef<Path>) {
    *(CRYPTPILOT_CONFIG_DIR.write().await) = PathBuf::from(config_dir.as_ref());
}

pub async fn get_config_dir() -> PathBuf {
    CRYPTPILOT_CONFIG_DIR.read().await.clone()
}

pub async fn load_volume_configs() -> Result<Vec<VolumeConfig>> {
    let mut volume_configs = Vec::new();
    let config_dir = get_config_dir().await.join("volumes");

    debug!("Loading volume configs from: {config_dir:?}");
    if !config_dir.exists() {
        bail!("Directory not found: {}", config_dir.display());
    }

    let mut volume_names = HashSet::<String>::new();

    let mut entries = tokio::fs::read_dir(config_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "conf") {
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

    Ok(volume_configs)
}

pub async fn load_volume_config(volume: &str) -> Result<VolumeConfig> {
    crate::config::load_volume_configs()
        .await
        .and_then(|volume_configs| {
            let volume_config = volume_configs
                .into_iter()
                .find(|volume_config| volume_config.volume == volume)
                .ok_or_else(|| anyhow!("Unknown volume name: {volume}"))?;

            Ok(volume_config)
        })
        .with_context(|| format!("Failed to load config for volume name: {}", volume))
}

#[cfg(test)]
pub mod tests {

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
