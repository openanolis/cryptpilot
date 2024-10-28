use std::{io::Write, path::PathBuf};

use anyhow::{bail, Context, Result};
use run_script::ScriptOptions;
use tokio::fs::OpenOptions;

use crate::{
    config::volume::MakeFsType,
    types::{IntegrityType, Passphrase},
};

use super::{get_verbose, shell::Shell};

pub async fn format(dev: &str, passphrase: &Passphrase, integrity: IntegrityType) -> Result<()> {
    let dev = dev.to_owned();
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut passphrase_file = tempfile::Builder::new()
            .tempfile()
            .context("Failed to create temp file for passphrase")?;

        passphrase_file.write_all(passphrase.as_bytes())?;

        Shell(format!(
            r#"
            cat {:?} | cryptsetup {} luksFormat --type luks2 --cipher aes-xts-plain64 {} {} -
            "#,
            passphrase_file.path(),
            match verbose {
                true => "--debug",
                false => "",
            },
            match integrity {
                IntegrityType::None => "",
                IntegrityType::Journal => "--integrity hmac-sha256 --integrity-no-wipe",
                IntegrityType::NoJournal =>
                    "--integrity hmac-sha256 --integrity-no-wipe --integrity-no-journal",
            },
            dev
        ))
        .run()
        .with_context(|| format!("Failed to format {dev} as LUKS2 volume"))
    })
    .await
    .context("background task failed")??;

    Ok(())
}

pub async fn open(
    volume: &str,
    dev: &str,
    passphrase: &Passphrase,
    integrity: IntegrityType,
) -> Result<(), anyhow::Error> {
    let dev = dev.to_owned();
    let volume = volume.to_owned();
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    tokio::task::spawn_blocking(move || -> Result<_> {
        let mut passphrase_file = tempfile::Builder::new()
            .tempfile()
            .context("Failed to create temp file for passphrase")?;

        passphrase_file.write_all(passphrase.as_bytes())?;

        Shell(format!(
            r#"
            cat {:?} | cryptsetup {} open --type luks2 {} --key-file=- {} {}
            "#,
            passphrase_file.path(),
            match verbose {
                true => "--debug",
                false => "",
            },
            match integrity {
                IntegrityType::None | IntegrityType::Journal => format!(""),
                IntegrityType::NoJournal => format!("--integrity-no-journal"),
            },
            dev,
            volume
        ))
        .run()
        .with_context(|| format!("Failed to setup mapping for volume {}", volume))
    })
    .await
    .context("background task failed")??;

    Ok(())
}

pub async fn is_initialized(dev: &str) -> Result<bool> {
    let dev = dev.to_owned();
    tokio::task::spawn_blocking(move || -> Result<_> {
        Shell(format!(
            r#"
                cryptsetup isLuks {}
                "#,
            dev
        ))
        .run_with_status_checker(|code, _, _| {
            let initialized = match code {
                0 => true,
                1 => false,
                _ => {
                    bail!("Bad exit code")
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
    let verbose = get_verbose().await;

    let mut ops = ScriptOptions::new();
    ops.exit_on_error = true;
    Shell(format!(
        r#"
            cryptsetup {} close {volume}
         "#,
        match verbose {
            true => "--debug",
            false => "",
        },
    ))
    .run()
    .with_context(|| format!("Failed to close mapping for volume `{volume}`"))?;
    Ok(())
}

pub async fn makefs_if_empty(
    volume: &str,
    makefs: &MakeFsType,
    integrity: IntegrityType,
) -> Result<()> {
    let volume = volume.to_owned();
    let makefs = makefs.to_owned();

    // There is no need to check volume here since systemd-makefs will check it.
    tokio::task::spawn_blocking(move || -> Result<_> {
        match integrity {
            // TODO: omit /usr/lib/systemd/systemd-makefs
            IntegrityType::None => Shell(format!(
                r#"
                        /usr/lib/systemd/systemd-makefs {} /dev/mapper/{}
                        "#,
                makefs.to_systemd_makefs_fstype(),
                volume,
            ))
            .run(),
            IntegrityType::Journal | IntegrityType::NoJournal => Shell(format!(
                r#"
                        export LC_ALL=C
                        set +o errexit
                        res=`file -E --brief --dereference --special-files /dev/mapper/{}`
                        status=$?
                        set -o errexit

                        if [[ $res == *"Input/output error"* ]] || [[ $res == "data" ]] ; then
                            # A uninitialized (empty) volume
                            exit 2
                        elif [[ $status -ne 0 ]] ; then
                            # Error happens
                            echo $res >&2
                            exit 1
                        else
                            # Maybe some thing on the volume, so we should not touch it.
                            exit 3
                        fi
                        "#,
                volume,
            ))
            .run_with_status_checker(|code, _, _| match code {
                2 => Ok(true),
                3 => Ok(false),
                _ => {
                    bail!("Bad exit code")
                }
            })
            .with_context(|| format!("Failed to detecting filesystem type on volume {volume}",))
            .and_then(|empty_volume| {
                if empty_volume {
                    makefs.mkfs_on_no_wipe_volume_blocking(&format!("/dev/mapper/{volume}"))?
                }
                Ok(())
            }),
        }
        .with_context(|| format!("Failed to initialize {makefs} fs on volume {volume}"))?;
        Ok(())
    })
    .await
    .context("background task failed")?
}
