use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use log::info;
use rand::RngCore as _;
use run_script::ScriptOptions;

use crate::cli::OpenOptions;

const GENERATED_PASSPHRASE_LEN: usize = 64;

pub async fn cmd_open(open_options: &OpenOptions) -> Result<()> {
    let volume_config = crate::config::load_volume_config(&open_options.volume).await?;

    info!(
        "The key_provider type is \"{}\"",
        serde_variant::to_variant_name(&volume_config.key_provider)?
    );

    if PathBuf::from(format!("/dev/mapper/{}", volume_config.volume)).exists() {
        info!("The mapping for {} already exists", volume_config.volume);
        return Ok(());
    }

    match volume_config.key_provider {
        crate::config::KeyProviderOptions::Temp(_temp_options) => {
            tokio::task::spawn_blocking(move || -> Result<_> {
                // TODO: store passphrase with auto clean container
                let mut passphrase = [0u8; GENERATED_PASSPHRASE_LEN / 2];
                let mut rng = rand::thread_rng();
                rng.fill_bytes(&mut passphrase);
                let passphrase = hex::encode(passphrase);
                info!("Generated temporary passphrase: {passphrase}");

                info!("Formatting and setting up mapping now");
                let mut ops = ScriptOptions::new();
                ops.exit_on_error = true;
                run_script::run_script!(
                    format!(
                        r#"
                        echo -n {passphrase} | cryptsetup luksFormat --type luks2 {} -
                        echo -n {passphrase} | cryptsetup open {} {}
                     "#,
                        volume_config.dev, volume_config.dev, volume_config.volume
                    ),
                    ops
                )
                .map_err(Into::into)
                .and_then(|(code, output, error)| {
                    if code != 0 {
                        bail!("Bad exit code: {code}\n\tstdout: {output}\n\tstderr: {error}")
                    } else {
                        Ok((output, error))
                    }
                })
                .with_context(|| {
                    format!(
                        "Failed to setup LUKS2 for volume `{}`",
                        volume_config.volume
                    )
                })?;

                Ok(passphrase)
            })
            .await
            .context("background task failed")??;
        }
        crate::config::KeyProviderOptions::Kms(kms_options) => todo!(),
        crate::config::KeyProviderOptions::Kbs(kbs_options) => todo!(),
        crate::config::KeyProviderOptions::Tpm2(tpm2_options) => todo!(),
    }

    info!("The mapping is ready now");

    Ok(())
}
