use anyhow::{Context, Result};
use log::info;

use crate::cli::InitOptions;

pub async fn cmd_init(init_options: &InitOptions) -> Result<()> {
    let volume_config = crate::config::load_volume_config(&init_options.volume).await?;

    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );
    match volume_config.key_provider {
        crate::config::KeyProviderOptions::Temp(_temp_options) => {
            info!("No need to initialize")
        }
        crate::config::KeyProviderOptions::Kms(kms_options) => todo!(),
        crate::config::KeyProviderOptions::Kbs(kbs_options) => todo!(),
        crate::config::KeyProviderOptions::Tpm2(tpm2_options) => todo!(),
    }

    Ok(())
}
