use anyhow::{bail, Result};
use dialoguer::Confirm;
use log::{error, info};
use rand::{distributions::Alphanumeric, Rng as _};

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
            if crate::fs::luks2::is_initialized(&volume_config.dev).await?
                && !init_options.force_reinit
            {
                bail!("The device {} is already initialized. Use '--force-reinit' to force re-initialize the volume.", volume_config.dev);
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

            if crate::fs::luks2::is_dev_in_use(&volume_config.dev).await? {
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
            crate::fs::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

            if let Some(makefs) = volume_config.extra_options.makefs {
                let tmp_volume_name = format!(
                    ".{}-{}",
                    volume_config.volume,
                    rand::thread_rng()
                        .sample_iter(&Alphanumeric)
                        .take(20)
                        .map(char::from)
                        .collect::<String>()
                );

                info!("Setting up a temporary device-mapper volume {tmp_volume_name}",);
                crate::fs::luks2::open(
                    &tmp_volume_name,
                    &volume_config.dev,
                    &passphrase,
                    integrity,
                )
                .await?;

                info!(
                    "Initializing {makefs} fs on volume {}",
                    volume_config.volume
                );
                match crate::fs::luks2::makefs_if_empty(&tmp_volume_name, &makefs, integrity).await
                {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Closing temporary volume {tmp_volume_name} now since: {e:#}");
                    }
                };
                crate::fs::luks2::close(&tmp_volume_name).await?;
            }
        }
        KeyProviderOptions::Kbs(_kbs_options) => todo!(),
        KeyProviderOptions::Tpm2(_tpm2_options) => todo!(),
    }

    info!("The volume is initialized now");

    Ok(())
}
