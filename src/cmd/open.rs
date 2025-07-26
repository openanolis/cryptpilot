use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;

use crate::{
    cli::OpenOptions,
    config::volume::VolumeConfig,
    provider::{IntoProvider, KeyProvider},
    types::IntegrityType,
};

pub struct OpenCommand {
    pub open_options: OpenOptions,
}

#[async_trait]
impl super::Command for OpenCommand {
    async fn run(&self) -> Result<()> {
        for volume in &self.open_options.volume {
            tracing::info!("Open volume {volume} now");
            let volume_config = crate::config::source::get_config_source()
                .await
                .get_volume_config(&volume)
                .await?;

            open_for_specific_volume(&volume_config).await?;
            tracing::info!("The volume {volume} is active now");
        }
        Ok(())
    }
}

pub async fn open_for_specific_volume(volume_config: &VolumeConfig) -> Result<()> {
    tracing::info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.encrypt.key_provider)?
    );
    if crate::fs::luks2::is_active(&volume_config.volume) {
        tracing::info!("The mapping for {} already exists", volume_config.volume);
        return Ok(());
    }
    if crate::fs::luks2::is_dev_in_use(&volume_config.dev).await? {
        bail!("The device {} is currently in use", volume_config.dev);
    }

    let key_provider = volume_config.encrypt.key_provider.clone().into_provider();
    let volume_config = volume_config.to_owned();

    Ok(match key_provider.volume_type() {
        crate::provider::VolumeType::Temporary => {
            temporary_disk_open(&volume_config, &key_provider).await?;
        }
        crate::provider::VolumeType::Persistent => {
            persistent_disk_open(&volume_config, &key_provider).await?;
        }
    })
}

async fn temporary_disk_open(
    volume_config: &VolumeConfig,
    key_provider: &impl KeyProvider,
) -> Result<()> {
    let passphrase = key_provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;
    tracing::info!("The temporary passphrase generated");

    tracing::info!("Formatting {} as LUKS2 volume now", volume_config.dev);
    let integrity = match volume_config.extra_config.integrity {
        Some(true) => IntegrityType::NoJournal,
        Some(false) | None => IntegrityType::None,
    };
    crate::fs::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

    tracing::info!("Setting up mapping for volume {} now", volume_config.volume);
    crate::fs::luks2::open_with_check_passphrase(
        &volume_config.volume,
        &volume_config.dev,
        &passphrase,
        integrity,
    )
    .await?;

    if let Some(makefs) = &volume_config.extra_config.makefs {
        tracing::info!(
            "Initializing {makefs} fs on volume {}",
            volume_config.volume
        );
        match crate::fs::luks2::makefs_if_empty(&volume_config.volume, &makefs, integrity).await {
            Ok(_) => (),
            Err(e) => {
                tracing::info!("Closing volume {} now", volume_config.volume);
                crate::fs::luks2::close(&volume_config.volume).await?;
                Err(e)?
            }
        };
    }
    Ok(())
}

async fn persistent_disk_open(
    volume_config: &VolumeConfig,
    key_provider: &impl KeyProvider,
) -> Result<()> {
    if !crate::fs::luks2::is_initialized(&volume_config.dev).await? {
        bail!(
            "{} is not a valid LUKS2 volume, should be initialized before opening it",
            volume_config.dev
        );
    }

    tracing::info!("Fetching passphrase for volume {}", volume_config.volume);
    let passphrase = key_provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;

    tracing::info!("Setting up mapping for volume {} now", volume_config.volume);
    let integrity = match volume_config.extra_config.integrity {
        Some(true) => IntegrityType::NoJournal,
        Some(false) | None => IntegrityType::None,
    };
    crate::fs::luks2::open_with_check_passphrase(
        &volume_config.volume,
        &volume_config.dev,
        &passphrase,
        integrity,
    )
    .await?;
    Ok(())
}
