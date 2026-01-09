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

        let config_source = crate::config::get_fde_config_source().await;

        // Check if config can be loaded
        tracing::info!(
            "Load FDE config from {}",
            config_source.source_debug_string()
        );
        let global_config = config_source
            .get_global_config()
            .await
            .context("Load global config failed")?;
        let fde_config = config_source
            .get_fde_config()
            .await
            .context("Load FDE config failed")?;

        async {
            // Check for the FDE config
            if let Some(fde) = fde_config {
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

            // Check global config
            if let Some(_global) = global_config {
                tracing::info!("Global config loaded successfully");
            } else {
                tracing::info!("No global config found");
            }

            Ok(())
        }.instrument(tracing::info_span!("check-fde")).await?;

        if is_error {
            bail!("FDE config check failed, please check the errors above")
        } else {
            tracing::info!("FDE config check passed")
        }

        Ok(())
    }
}
