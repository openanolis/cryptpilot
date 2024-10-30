use anyhow::{bail, Result};
use log::info;

use crate::{
    cli::OpenOptions,
    config::VolumeConfig,
    luks2,
    provider::{IntoProvider, KeyProvider as _},
};

pub async fn cmd_open(open_options: &OpenOptions) -> Result<()> {
    let volume_config = crate::config::load_volume_config(&open_options.volume).await?;

    open_for_specific_volume(&volume_config).await?;

    info!("The mapping is active now");

    Ok(())
}

pub async fn open_for_specific_volume(volume_config: &VolumeConfig) -> Result<()> {
    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );
    if luks2::is_active(&volume_config.volume) {
        info!("The mapping for {} already exists", volume_config.volume);
        return Ok(());
    }
    if crate::luks2::is_dev_in_use(&volume_config.dev).await? {
        bail!("The device {} is currently in use", volume_config.dev);
    }
    let volume_config = volume_config.to_owned();
    Ok(match volume_config.key_provider {
        crate::config::KeyProviderOptions::Otp(otp_options) => {
            let provider = otp_options.into_provider();
            let passphrase = provider.get_key().await?;
            info!("Generated temporary passphrase: {passphrase:?}");

            crate::luks2::format(&volume_config.dev, &passphrase).await?;
            crate::luks2::open(&volume_config.volume, &volume_config.dev, &passphrase).await?;
        }
        crate::config::KeyProviderOptions::Kms(kms_options) => {
            if !crate::luks2::is_initialized(&volume_config.dev).await? {
                bail!(
                    "{} is not a valid LUKS2 volume, should be initialized before opening it",
                    volume_config.dev
                );
            }

            info!("Fetching passphrase for volume {}", volume_config.volume);
            let provider = kms_options.into_provider();
            let passphrase = provider.get_key().await?;

            crate::luks2::open(&volume_config.volume, &volume_config.dev, &passphrase).await?;
        }
        crate::config::KeyProviderOptions::Kbs(_kbs_options) => todo!(),
        crate::config::KeyProviderOptions::Tpm2(_tpm2_options) => todo!(),
    })
}
