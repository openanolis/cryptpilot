use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;

use crate::cli::OpenOptions;
use cryptpilot::{
    provider::{IntoProvider, KeyProvider},
    types::IntegrityType,
};

use crate::config::VolumeConfig;

pub struct OpenCommand {
    pub open_options: OpenOptions,
}

#[async_trait]
impl crate::cmd::Command for OpenCommand {
    async fn run(&self) -> Result<()> {
        for volume in &self.open_options.volume {
            tracing::info!("Open volume {volume} now");
            let volume_config = crate::config::get_volume_config_source()
                .await
                .get_volume_config(volume)
                .await?;

            open_for_specific_volume(&volume_config, self.open_options.check_fs).await?;
            tracing::info!("The volume {volume} is active now");
        }
        Ok(())
    }
}

pub async fn open_for_specific_volume(volume_config: &VolumeConfig, check_fs: bool) -> Result<()> {
    tracing::info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.encrypt.key_provider)?
    );
    if cryptpilot::fs::luks2::is_active(&volume_config.volume) {
        tracing::info!("The mapping for {} already exists", volume_config.volume);
        return Ok(());
    }
    if cryptpilot::fs::luks2::is_dev_in_use(&volume_config.dev).await? {
        bail!("The device {} is currently in use", volume_config.dev);
    }

    let key_provider = volume_config.encrypt.key_provider.clone().into_provider();
    let volume_config = volume_config.to_owned();

    match key_provider.volume_type() {
        cryptpilot::provider::VolumeType::Temporary => {
            temporary_disk_open(&volume_config, &key_provider).await?;
        }
        cryptpilot::provider::VolumeType::Persistent => {
            persistent_disk_open(&volume_config, &key_provider).await?;
        }
    };

    // Check if filesystem is ready
    if check_fs
        && volume_config.extra_config.makefs.is_some()
        && cryptpilot::fs::mkfs::is_empty_disk(&volume_config.volume_path()).await?
    {
        // TODO: replace with RAII here
        let _ = cryptpilot::fs::luks2::close(&volume_config.volume).await;
        bail!(
            "The filesystem on {:?} is not initialized but makefs is set, the volume maybe not fully initialized. Try running `cryptpilot-crypt init` again with `--force-reinit`",
            volume_config.volume_path()
        )
    }

    Ok(())
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
    cryptpilot::fs::luks2::format(&volume_config.dev, &passphrase, integrity).await?;

    tracing::info!("Setting up mapping for volume {} now", volume_config.volume);
    cryptpilot::fs::luks2::open_with_check_passphrase(
        &volume_config.volume,
        &volume_config.dev,
        &passphrase,
        integrity,
    )
    .await?;

    if let Some(makefs) = &volume_config.extra_config.makefs {
        match cryptpilot::fs::mkfs::makefs_if_empty(&volume_config.volume_path(), makefs, integrity)
            .await
        {
            Ok(_) => (),
            Err(e) => {
                tracing::info!("Closing volume {} now", volume_config.volume);
                cryptpilot::fs::luks2::close(&volume_config.volume).await?;
                Err(e)?
            }
        };
    }

    // Mark the volume as fully initialized
    cryptpilot::fs::luks2::mark_volume_as_initialized(std::path::Path::new(&volume_config.dev))
        .await?;

    Ok(())
}

async fn persistent_disk_open(
    volume_config: &VolumeConfig,
    key_provider: &impl KeyProvider,
) -> Result<()> {
    if !cryptpilot::fs::luks2::is_initialized(&volume_config.dev).await? {
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
    cryptpilot::fs::luks2::open_with_check_passphrase(
        &volume_config.volume,
        &volume_config.dev,
        &passphrase,
        integrity,
    )
    .await?;

    Ok(())
}
