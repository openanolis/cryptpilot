use std::path::Path;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tracing::Instrument;

use crate::{
    cli::ConfigCheckOptions,
    provider::{IntoProvider, KeyProvider},
};

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

        let config_source = crate::config::source::get_config_source().await;

        // Check if config can be loaded
        tracing::info!("Load config from {}", config_source.source_debug_string());
        let config = config_source
            .get_config()
            .await
            .context("Load config failed")?;

        // Check for each volume config
        async {
            tracing::info!("Checking configs for {} volumes", config.volumes.len());
        }
        .instrument(tracing::info_span!("check-volume"))
        .await;

        for volume in config.volumes.iter() {
            async {
                tracing::info!("Checking config for volume \"{}\"", volume.volume);

                let mut dev_is_initialized = false;

                // Check if the device exists
                if !Path::new(&volume.dev).exists() {
                    continue_or_throw!(
                        "The device \"{}\" for volume \"{}\" does not exist",
                        volume.dev,
                        volume.volume
                    );
                } else {
                    // Check if the device is initialized when it is auto open
                    match crate::fs::luks2::is_initialized(&volume.dev)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to check if the device \"{}\" is initialized",
                                volume.dev
                            )
                        }) {
                        Ok(initialized) => {
                            if volume.extra_config.auto_open == Some(true) && !initialized {
                                continue_or_throw!("The volume \"{}\" is set to auto open but the device \"{}\" is not initialized", volume.volume, volume.dev);
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
                                match crate::fs::luks2::check_passphrase(&volume.dev, &passphrase)
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

        async{
            // Check for the FDE config
            if let Some(fde) = config.fde {
                if fde.rootfs.encrypt.is_none() {
                    tracing::info!("Encryption is not enabled for FDE rootfs volume, skipping");
                }

                let encrypt_configs = fde
                    .rootfs
                    .encrypt
                    .map(|encrypt| ("rootfs", encrypt))
                    .into_iter()
                    .chain(std::iter::once(("data", fde.data.encrypt)));

                for (volume_debug_name, encrypt) in encrypt_configs {
                    tracing::info!("Checking config for FDE \"{}\" volume", volume_debug_name);

                    if self.config_check_options.skip_check_passphrase {
                        tracing::warn!("Skipping key check for FDE volume \"{}\" due to \"--skip-check-passphrase\" is set", volume_debug_name);
                } else {
                        let key_provider = encrypt.key_provider.clone().into_provider();
                        match key_provider.get_key().await.with_context(|| {
                            format!(
                                "Failed to get key for FDE \"{}\" volume from key provider {}",
                                volume_debug_name,
                                key_provider.debug_name()
                            )
                        }) {
                            Ok(_passphrase) => {
                                tracing::info!(
                                    "Key for FDE \"{}\" volume is fetched successfully",
                                    volume_debug_name,
                                );
                            }
                            Err(error) => continue_or_throw!(error),
                        }
                    }
                }
            } else {
                tracing::info!("No FDE config found, skipping");
            }
            Ok(())
        }.instrument(tracing::info_span!("check-fde")).await?;

        if is_error {
            bail!("Config check failed, please check the errors above")
        } else {
            tracing::info!("Config check passed")
        }

        Ok(())
    }
}
