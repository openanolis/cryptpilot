use anyhow::{bail, Result};
use log::{error, info};

use crate::{
    cli::OpenOptions,
    config::volume::{KeyProviderOptions, VolumeConfig},
    provider::{IntoProvider, KeyProvider as _},
    types::IntegrityType,
};

pub async fn cmd_open(open_options: &OpenOptions) -> Result<()> {
    let volume_config = crate::config::volume::load_volume_config(&open_options.volume).await?;

    open_for_specific_volume(&volume_config).await?;

    info!("The mapping is active now");

    Ok(())
}

pub async fn open_for_specific_volume(volume_config: &VolumeConfig) -> Result<()> {
    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );
    if crate::fs::luks2::is_active(&volume_config.volume) {
        info!("The mapping for {} already exists", volume_config.volume);
        return Ok(());
    }
    if crate::fs::luks2::is_dev_in_use(&volume_config.dev).await? {
        bail!("The device {} is currently in use", volume_config.dev);
    }
    let volume_config = volume_config.to_owned();
    Ok(match volume_config.key_provider {
        KeyProviderOptions::Otp(otp_options) => {
            let provider = otp_options.into_provider();
            let passphrase = provider.get_key().await?;
            info!("Generated temporary passphrase: {passphrase:?}");

            info!("Formatting {} as LUKS2 volume now", volume_config.dev);
            let integrity = match volume_config.extra_options.integrity {
                Some(true) => IntegrityType::NoJournal,
                Some(false) | None => IntegrityType::None,
            };
            crate::fs::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

            info!("Setting up mapping for volume {} now", volume_config.volume);
            crate::fs::luks2::open(
                &volume_config.volume,
                &volume_config.dev,
                &passphrase,
                integrity,
            )
            .await?;

            if let Some(makefs) = volume_config.extra_options.makefs {
                info!(
                    "Initializing {makefs} fs on volume {}",
                    volume_config.volume
                );
                match crate::fs::luks2::makefs_if_empty(&volume_config.volume, &makefs, integrity)
                    .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        error!("Closing volume {} now since: {e:#}", volume_config.volume);
                        crate::fs::luks2::close(&volume_config.volume).await?;
                    }
                };
            }
        }
        KeyProviderOptions::Kms(kms_options) => {
            if !crate::fs::luks2::is_initialized(&volume_config.dev).await? {
                bail!(
                    "{} is not a valid LUKS2 volume, should be initialized before opening it",
                    volume_config.dev
                );
            }

            info!("Fetching passphrase for volume {}", volume_config.volume);
            let provider = kms_options.into_provider();
            let passphrase = provider.get_key().await?;

            info!("Setting up mapping for volume {} now", volume_config.volume);
            let integrity = match volume_config.extra_options.integrity {
                Some(true) => IntegrityType::NoJournal,
                Some(false) | None => IntegrityType::None,
            };
            crate::fs::luks2::open(
                &volume_config.volume,
                &volume_config.dev,
                &passphrase,
                integrity,
            )
            .await?;
        }
        KeyProviderOptions::Kbs(_kbs_options) => todo!(),
        KeyProviderOptions::Tpm2(_tpm2_options) => todo!(),
    })
}
