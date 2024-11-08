use anyhow::{Context as _, Result};
use lazy_static::lazy_static;
use log::debug;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::get_config_dir;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    /// Configuration related to cryptpilot.service systemd service.
    #[serde(default = "Default::default")]
    pub systemd: SystemdConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SystemdConfig {
    /// Enable this option if you want more log when running cryptpilot.service systemd service.
    #[serde(default = "Default::default")]
    pub verbose: bool,
}

lazy_static! {
    static ref CRYPTPILOT_GLOBAL_CONFIG: RwLock<Option<GlobalConfig>> = RwLock::new(None);
}

pub async fn get_global_config() -> Result<GlobalConfig> {
    let read = CRYPTPILOT_GLOBAL_CONFIG.read().await;
    match &*read {
        None => {
            drop(read);

            let mut write: tokio::sync::RwLockWriteGuard<'_, Option<GlobalConfig>> =
                CRYPTPILOT_GLOBAL_CONFIG.write().await;
            // Double check
            match &*write {
                None => {
                    let config_path = get_config_dir().await.join("cryptpilot.toml");

                    debug!("Loading cryptpilot config from: {config_path:?}");

                    let global_config = tokio::fs::read_to_string(&config_path)
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|content| {
                            toml::from_str::<GlobalConfig>(&content)
                                .context("Failed to parse content as TOML")
                        })
                        .with_context(|| {
                            format!("Failed to load cryptpilot config from: {config_path:?}")
                        })?;

                    *write = Some(global_config.clone());
                    Ok(global_config)
                }
                Some(v) => Ok(v.clone()),
            }
        }
        Some(v) => Ok(v.clone()),
    }
}

#[cfg(test)]
pub mod tests {

    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_deserialize_empty_config() -> Result<()> {
        let raw = "";

        let config: GlobalConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            GlobalConfig {
                systemd: SystemdConfig { verbose: false }
            }
        );

        let raw = r#"
        [systemd]
        "#;
        let config: GlobalConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            GlobalConfig {
                systemd: SystemdConfig { verbose: false }
            }
        );
        Ok(())
    }

    #[test]
    fn test_deserialize_wrong_config() -> Result<()> {
        let raw = r#"
        [systemdddddd]
        "#;
        assert!(toml::from_str::<GlobalConfig>(raw).is_err());

        let raw = r#"
        [systemd]
        [systemd]
        "#;
        assert!(toml::from_str::<GlobalConfig>(raw).is_err());

        Ok(())
    }
}
