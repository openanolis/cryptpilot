use anyhow::{bail, Context, Result};
use log::info;
use run_script::ScriptOptions;

use crate::{
    cli::InitOptions,
    provider::{kms::KmsKeyProvider, IntoProvider, KeyProvider as _},
};

pub async fn cmd_init(init_options: &InitOptions) -> Result<()> {
    let volume_config = crate::config::load_volume_config(&init_options.volume).await?;

    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );
    match volume_config.key_provider {
        crate::config::KeyProviderOptions::Temp(_temp_options) => {
            info!("No need to initialize");
            return Ok(());
        }
        crate::config::KeyProviderOptions::Kms(kms_options) => {
            info!("Fetching passphrase for volume {}", volume_config.volume);
            let provider = kms_options.into_provider();
            let passphrase = provider.get_key().await?;

            crate::luks2::format(&volume_config.dev, &passphrase).await?;
        }
        crate::config::KeyProviderOptions::Kbs(kbs_options) => todo!(),
        crate::config::KeyProviderOptions::Tpm2(tpm2_options) => todo!(),
    }

    info!("The volume is initialized now");

    Ok(())
}
