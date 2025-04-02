use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use tokio::{fs::OpenOptions, process::Command};

use crate::{
    config::volume::MakeFsType,
    types::{IntegrityType, Passphrase},
};

use super::{
    cmd::CheckCommandOutput as _,
    get_verbose,
    mkfs::{IntegrityNoWipeMakeFs, MakeFs, NormalMakeFs},
};

pub async fn format(dev: &str, passphrase: &Passphrase, integrity: IntegrityType) -> Result<()> {
    let dev = dev.to_owned();
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    let mut cmd = Command::new("cryptsetup");
    if verbose {
        cmd.arg("--debug");
    }

    cmd.args([
        "luksFormat",
        "--type",
        "luks2",
        "--cipher",
        "aes-xts-plain64",
    ]);

    match integrity {
        IntegrityType::None => {}
        IntegrityType::Journal => {
            cmd.args(["--integrity", "hmac-sha256", "--integrity-no-wipe"]);
        }
        IntegrityType::NoJournal => {
            cmd.args([
                "--integrity",
                "hmac-sha256",
                "--integrity-no-wipe",
                "--integrity-no-journal",
            ]);
        }
    };

    cmd.arg(&dev).arg("-");

    cmd.run_with_input(Some(passphrase.as_bytes()))
        .await
        .with_context(|| format!("Failed to format {dev} as LUKS2 volume"))?;

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

    let mut cmd = Command::new("cryptsetup");
    if verbose {
        cmd.arg("--debug");
    }

    cmd.args(["open", "--type", "luks2"]);

    match integrity {
        IntegrityType::None | IntegrityType::Journal => {}
        IntegrityType::NoJournal => {
            cmd.args(["--integrity-no-journal"]);
        }
    };

    cmd.args(["--key-file=-"]);
    cmd.arg(dev).arg(&volume);

    cmd.run_with_input(Some(passphrase.as_bytes()))
        .await
        .with_context(|| format!("Failed to setup mapping for volume {volume}"))?;

    Ok(())
}

pub async fn is_initialized(dev: &str) -> Result<bool> {
    Command::new("cryptsetup")
        .arg("isLuks")
        .arg(dev)
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
        .await
        .with_context(|| format!("Failed to check initialization status of device {dev}"))
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

    let mut cmd = Command::new("cryptsetup");
    if verbose {
        cmd.arg("--debug");
    }
    cmd.arg("close")
        .arg(volume)
        .run()
        .await
        .with_context(|| format!("Failed to close volume `{volume}`"))?;

    Ok(())
}

pub async fn makefs_if_empty(
    volume: &str,
    makefs: &MakeFsType,
    integrity: IntegrityType,
) -> Result<()> {
    let volume = volume.to_owned();
    let makefs = makefs.to_owned();

    let device_path = format!("/dev/mapper/{volume}");
    match integrity {
        IntegrityType::None => NormalMakeFs::mkfs(device_path, makefs).await,
        IntegrityType::Journal | IntegrityType::NoJournal => {
            IntegrityNoWipeMakeFs::mkfs(device_path, makefs).await
        }
    }
    .with_context(|| format!("Failed to initialize {makefs} fs on volume {volume}"))?;
    Ok(())
}
