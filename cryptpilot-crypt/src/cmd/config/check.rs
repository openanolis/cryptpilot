use std::path::Path;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tracing::Instrument;

use crate::cli::ConfigCheckOptions;
use cryptpilot::provider::{IntoProvider, KeyProvider};

pub struct ConfigCheckCommand {
    pub config_check_options: ConfigCheckOptions,
}

#[async_trait]
impl super::super::Command for ConfigCheckCommand {
    async fn run(&self) -> Result<()> {
        let mut is_error = false;

        macro_rules! continue_or_throw {
            ($error:expr) => ({
                is_error = true;
                if self.config_check_options.keep_checking {
                    tracing::error!(error=?$error);
                } else {
                    anyhow::bail!($error);
                }
            });
            ($($arg:tt)*) => ({
                is_error = true;

                if self.config_check_options.keep_checking {
                    tracing::error!($($arg)*);
                } else {
                    anyhow::bail!($($arg)*);
                }
            });
        }

        let config_source = crate::config::get_volume_config_source().await;

        // Check if config can be loaded
        tracing::info!("Load config from {}", config_source.source_debug_string());
        let volumes = config_source
            .get_volume_configs()
            .await
            .context("Load config failed")?;

        // Check for each volume config
        async {
            tracing::info!("Checking configs for {} volumes", volumes.len());
        }
        .instrument(tracing::info_span!("check-volume"))
        .await;

        for volume in volumes.iter() {
            async {
                tracing::info!("Checking config for volume \"{}\"", volume.volume);

                let mut dev_is_initialized = false;

                // Check if the device exists
                if !Path::new(&volume.dev).exists() {
                    continue_or_throw!(
                        "The device {:?} for volume \"{}\" does not exist",
                        volume.dev,
                        volume.volume
                    );
                } else {
                    // Check if the device is initialized when it is auto open
                    match cryptpilot::fs::luks2::is_initialized(&volume.dev)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to check if the device {:?} is initialized",
                                volume.dev
                            )
                        }) {
                        Ok(initialized) => {
                            if volume.extra_config.auto_open == Some(true) && !initialized {
                                continue_or_throw!("The volume \"{}\" is set to auto open but the device {:?} is not initialized", volume.volume, volume.dev);
                            }
                            dev_is_initialized = initialized;
                        }
                        Err(error) => continue_or_throw!(error),
                    }
                }

                if self.config_check_options.skip_check_passphrase {
                    tracing::warn!("Skipping key check for volume \"{}\" due to \"--skip-check-passphrase\" is set", volume.volume);
                } else {
                    // Check if the key provider can get the key
                    let key_provider = volume.encrypt.key_provider.clone().into_provider();
                    match key_provider.get_key().await.with_context(|| {
                        format!(
                            "Failed to get key for volume \"{}\" from key provider {}",
                            volume.volume,
                            key_provider.debug_name()
                        )
                    }) {
                        Ok(passphrase) => {
                            tracing::info!("Key for volume \"{}\" is fetched successfully", volume.volume);

                            if dev_is_initialized {
                                // Check if the passphrase is correct if the device is initialized
                                match cryptpilot::fs::luks2::check_passphrase(&volume.dev, &passphrase)
                                    .await
                                    .with_context(|| {
                                        format!("The passphrase for volume \"{}\" is incorrect", volume.volume)
                                    }) {
                                    Ok(()) => {
                                        tracing::info!(
                                            "The passphrase for volume \"{}\" is correct",
                                            volume.volume
                                        );
                                    }
                                    Err(error) => continue_or_throw!(error),
                                }
                            }
                        }
                        Err(error) => continue_or_throw!(error),
                    }
                }

                tracing::info!("Checking config for volume \"{}\" finished", volume.volume);
                Ok(())
            }.instrument(tracing::info_span!("check-volume", volume=&volume.volume)).await?;
        }

        if is_error {
            bail!("Volume config check failed, please check the errors above")
        } else {
            tracing::info!("Volume config check passed")
        }

        Ok(())
    }
}
