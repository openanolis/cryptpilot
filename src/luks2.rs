use anyhow::{bail, Context, Result};
use log::info;
use run_script::ScriptOptions;

use crate::types::Passphrase;

pub async fn format(dev: &str, passphrase: &Passphrase) -> Result<()> {
    info!("Formatting {dev} as LUKS2 volume now",);

    let dev = dev.to_owned();
    let passphrase = passphrase.to_owned();
    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;
        run_script::run_script!(
            format!(
                r#"
                echo -n {} | base64 -d | cryptsetup luksFormat --type luks2 {} -
                "#,
                passphrase.to_base64(),
                dev
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
        .with_context(|| format!("Failed to format {dev} as LUKS2 volume"))
    })
    .await
    .context("background task failed")??;

    Ok(())
}

pub async fn open(volume: &str, dev: &str, passphrase: &Passphrase) -> Result<(), anyhow::Error> {
    info!("Setting up mapping for volume {volume} now");

    let dev = dev.to_owned();
    let volume = volume.to_owned();
    let passphrase = passphrase.to_owned();
    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;
        run_script::run_script!(
            format!(
                r#"
                echo -n {} | base64 -d | cryptsetup open --type luks --key-file=- {} {}
                "#,
                passphrase.to_base64(),
                dev,
                volume
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
        .with_context(|| format!("Failed to setup mapping for volume {}", volume))
    })
    .await
    .context("background task failed")??;

    Ok(())
}

pub async fn is_initialized(dev: &str) -> Result<bool> {
    let dev = dev.to_owned();
    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;
        run_script::run_script!(
            format!(
                r#"
                cryptsetup isLuks {}
                "#,
                dev
            ),
            ops
        )
        .map_err(Into::into)
        .and_then(|(code, output, error)| {
            let initialized = match code {
                0 => true,
                1 => false,
                _ => {
                    bail!("Bad exit code: {code}\n\tstdout: {output}\n\tstderr: {error}");
                }
            };
            Ok(initialized)
        })
        .with_context(|| format!("Failed to check {dev} as LUKS2 volume"))
    })
    .await
    .context("background task failed")?
}
