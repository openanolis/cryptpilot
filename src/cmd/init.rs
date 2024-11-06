use anyhow::{bail, Result};
use dialoguer::Confirm;
use log::info;

use crate::{
    cli::InitOptions,
    config::volume::KeyProviderOptions,
    provider::{IntoProvider, KeyProvider as _},
    types::IntegrityType,
};

pub async fn cmd_init(init_options: &InitOptions) -> Result<()> {
    let volume_config = crate::config::volume::load_volume_config(&init_options.volume).await?;

    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );
    match volume_config.key_provider {
        KeyProviderOptions::Otp(_otp_options) => {
            info!("Not required to initialize");
            return Ok(());
        }
        KeyProviderOptions::Kms(kms_options) => {
            if crate::luks2::is_initialized(&volume_config.dev).await? && !init_options.force_reinit
            {
                bail!("The device {} is already initialized", volume_config.dev);
            }

            if !init_options.yes
                && !Confirm::new()
                    .with_prompt(format!(
                        "All of the data on {} will be lost. Do you want to continue?",
                        volume_config.dev
                    ))
                    .default(false)
                    .interact()?
            {
                bail!("Operation canceled");
            }

            if crate::luks2::is_dev_in_use(&volume_config.dev).await? {
                bail!("The device {} is currently in use", volume_config.dev);
            }

            info!("Fetching passphrase for volume {}", volume_config.volume);
            let provider = kms_options.into_provider();
            let passphrase = provider.get_key().await?;

            info!("Formatting {} as LUKS2 volume now", volume_config.dev);
            let integrity = match volume_config.extra_options.integrity {
                Some(true) => IntegrityType::Journal,
                Some(false) | None => IntegrityType::None,
            };
            crate::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

            if let Some(makefs) = volume_config.extra_options.makefs {
                info!(
                    "Initializing {} fs on volume {}",
                    serde_variant::to_variant_name(&makefs)?,
                    volume_config.volume
                );

                let tmp_volume_name = format!(".{}", volume_config.volume);
                crate::luks2::open(&tmp_volume_name, &volume_config.dev, &passphrase, integrity)
                    .await?;
                crate::luks2::makefs_if_empty(&tmp_volume_name, &makefs, integrity).await?;
                crate::luks2::close(&tmp_volume_name).await?;
            }
        }
        KeyProviderOptions::Kbs(_kbs_options) => todo!(),
        KeyProviderOptions::Tpm2(_tpm2_options) => todo!(),
    }

    info!("The volume is initialized now");

    Ok(())
}
