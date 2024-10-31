use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use run_script::ScriptOptions;
use tokio::fs::OpenOptions;

use crate::{config::MakeFsType, types::Passphrase};

pub async fn format(dev: &str, passphrase: &Passphrase) -> Result<()> {
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
        .with_context(|| format!("Failed to check initialization status of device {dev}"))
    })
    .await
    .context("background task failed")?
}

pub fn is_active(volume: &str) -> bool {
    PathBuf::from(format!("/dev/mapper/{}", volume)).exists()
}

pub async fn is_dev_in_use(dev: &str) -> Result<bool> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(libc::O_EXCL);
    match options.open(dev).await {
        Ok(_) => Ok(false),
        Err(e) if e.raw_os_error() == Some(libc::EBUSY) => Ok(true),
        Err(e) => Err(e.into()),
    }
}

pub async fn close(volume: &str) -> Result<()> {
    let mut ops = ScriptOptions::new();
    ops.exit_on_error = true;
    run_script::run_script!(
        format!(
            r#"
            cryptsetup close {volume}
         "#
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
    .with_context(|| format!("Failed to close mapping for volume `{volume}`"))?;
    Ok(())
}

pub async fn makefs_if_empty(volume: &str, makefs: &MakeFsType) -> Result<()> {
    let volume = volume.to_owned();
    let makefs = makefs.to_owned();

    // There is no need to check volume here since systemd-makefs will check it.
    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;

        run_script::run_script!(
            format!(
                r#"
                /usr/lib/systemd/systemd-makefs {} /dev/mapper/{}
                "#,
                makefs.to_systemd_makefs_fstype(),
                volume,
            ),
            ops
        )
        .with_context(|| {
            format!(
                "Failed to initialize {} fs on volume {volume}",
                serde_variant::to_variant_name(&makefs).unwrap_or("unknown")
            )
        })?;

        Ok(())
    })
    .await
    .context("background task failed")?
}
