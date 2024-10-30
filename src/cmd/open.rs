use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use log::info;
use rand::RngCore as _;
use run_script::ScriptOptions;

use crate::{
    cli::OpenOptions,
    luks2,
    provider::{kms::KmsKeyProvider, IntoProvider, KeyProvider as _},
};

pub async fn cmd_open(open_options: &OpenOptions) -> Result<()> {
    let volume_config = crate::config::load_volume_config(&open_options.volume).await?;

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

    match volume_config.key_provider {
        crate::config::KeyProviderOptions::Temp(temp_options) => {
            let provider = temp_options.into_provider();
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
        crate::config::KeyProviderOptions::Kbs(kbs_options) => todo!(),
        crate::config::KeyProviderOptions::Tpm2(tpm2_options) => todo!(),
    }

    info!("The mapping is active now");

    Ok(())
}
