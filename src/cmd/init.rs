use anyhow::{bail, Context, Result};
use dialoguer::Confirm;
use log::info;
use rand::{distributions::Alphanumeric, Rng as _};

use crate::{
    cli::InitOptions,
    config::{encrypt::KeyProviderConfig, volume::VolumeConfig},
    provider::{IntoProvider, KeyProvider},
    types::IntegrityType,
};

pub async fn cmd_init(init_options: &InitOptions) -> Result<()> {
    let volume_config = crate::config::source::get_config_source()
        .await
        .get_volume_config(&init_options.volume)
        .await?;

    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.encrypt.key_provider)?
    );
    match &volume_config.encrypt.key_provider {
        KeyProviderConfig::Otp(_otp_config) => {
            info!("Not required to initialize");
            return Ok(());
        }
        KeyProviderConfig::Kms(kms_config) => {
            persistent_disk_init(init_options, &volume_config, kms_config.clone()).await?;
        }
        KeyProviderConfig::Kbs(kbs_config) => {
            persistent_disk_init(init_options, &volume_config, kbs_config.clone()).await?;
        }
        KeyProviderConfig::Tpm2(_tpm2_config) => todo!(),
        KeyProviderConfig::Oidc(oidc_config) => {
            persistent_disk_init(init_options, &volume_config, oidc_config.clone()).await?
        }
    }

    info!("The volume is initialized now");

    Ok(())
}

async fn persistent_disk_init(
    init_options: &InitOptions,
    volume_config: &VolumeConfig,
    into_provider: impl IntoProvider,
) -> Result<()> {
    if crate::fs::luks2::is_initialized(&volume_config.dev).await? && !init_options.force_reinit {
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
    let provider = into_provider.into_provider();
    let passphrase = provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;

    info!("Formatting {} as LUKS2 volume now", volume_config.dev);
    let integrity = match volume_config.extra_config.integrity {
        Some(true) => IntegrityType::Journal,
        Some(false) | None => IntegrityType::None,
    };
    crate::fs::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

    if let Some(makefs) = &volume_config.extra_config.makefs {
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
        crate::fs::luks2::open(&tmp_volume_name, &volume_config.dev, &passphrase, integrity)
            .await?;

        info!(
            "Initializing {makefs} fs on volume {}",
            volume_config.volume
        );
        let mkfs_result =
            crate::fs::luks2::makefs_if_empty(&tmp_volume_name, &makefs, integrity).await;
        crate::fs::luks2::close(&tmp_volume_name).await?;
        mkfs_result?
    }

    Ok(())
}
