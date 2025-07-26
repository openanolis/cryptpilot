use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use dialoguer::Confirm;
use rand::{distributions::Alphanumeric, Rng as _};

use crate::{
    cli::InitOptions,
    config::volume::VolumeConfig,
    provider::{IntoProvider, KeyProvider},
    types::IntegrityType,
};

pub struct InitCommand {
    pub init_options: InitOptions,
}

#[async_trait]
impl super::Command for InitCommand {
    async fn run(&self) -> Result<()> {
        for volume in &self.init_options.volume {
            tracing::info!("Initialize volume {volume} now");

            let volume_config = crate::config::source::get_config_source()
                .await
                .get_volume_config(&volume)
                .await?;

            tracing::info!(
                "The key_provider type is \"{}\"",
                serde_variant::to_variant_name(&volume_config.encrypt.key_provider)?
            );

            let key_provider = volume_config.encrypt.key_provider.clone().into_provider();

            match key_provider.volume_type() {
                crate::provider::VolumeType::Temporary => {
                    tracing::info!("Not required to initialize");
                    continue;
                }
                crate::provider::VolumeType::Persistent => {
                    persistent_disk_init(&self.init_options, &volume_config, &key_provider).await?;
                }
            }

            tracing::info!("The volume {volume} is initialized now");
        }
        Ok(())
    }
}

async fn persistent_disk_init(
    init_options: &InitOptions,
    volume_config: &VolumeConfig,
    key_provider: &impl KeyProvider,
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

    tracing::info!("Fetching passphrase for volume {}", volume_config.volume);
    let passphrase = key_provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;

    tracing::info!("Formatting {} as LUKS2 volume now", volume_config.dev);
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

        tracing::info!("Setting up a temporary device-mapper volume {tmp_volume_name}",);
        crate::fs::luks2::open_with_check_passphrase(&tmp_volume_name, &volume_config.dev, &passphrase, integrity)
            .await?;

        tracing::info!(
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
