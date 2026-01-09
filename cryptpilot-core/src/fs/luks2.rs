use std::path::PathBuf;

use anyhow::{Context, Result};
use libcryptsetup_rs::{
    consts::{
        flags::{CryptActivate, CryptDeactivate, CryptVolumeKey},
        vals::{CryptDebugLevel, EncryptionFormat},
    },
    CryptInit, CryptParamsLuks2, CryptParamsLuks2Ref,
};
use tokio::fs::OpenOptions;

use crate::types::{IntegrityType, MakeFsType, Passphrase};

use super::{
    get_verbose,
    mkfs::{IntegrityNoWipeMakeFs, MakeFs, NormalMakeFs},
};

const LUKS2_VOLUME_KEY_SIZE_BIT_WITH_INTEGRITY: usize = 768;
const LUKS2_VOLUME_KEY_SIZE_BIT_WITHOUT_INTEGRITY: usize = 512;
const LUKS2_SECTOR_SIZE: u32 = 4096;

pub async fn format(dev: &str, passphrase: &Passphrase, integrity: IntegrityType) -> Result<()> {
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    let device_path = PathBuf::from(&dev);

    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let params = CryptParamsLuks2 {
            integrity: Some("hmac(sha256)".to_owned()),
            pbkdf: None,
            integrity_params: None,
            data_alignment: 0,
            data_device: None,
            sector_size: LUKS2_SECTOR_SIZE,
            label: None,
            subsystem: None,
        };
        let mut params_ref = (&params).try_into()?;

        let volume_key = match integrity {
            IntegrityType::None => {
                libcryptsetup_rs::Either::Right(LUKS2_VOLUME_KEY_SIZE_BIT_WITHOUT_INTEGRITY / 8)
            }
            IntegrityType::Journal | IntegrityType::NoJournal => {
                libcryptsetup_rs::Either::Right(LUKS2_VOLUME_KEY_SIZE_BIT_WITH_INTEGRITY / 8)
            }
        };

        let mut device = CryptInit::init(&device_path)?;

        device.context_handle().format::<CryptParamsLuks2Ref>(
            EncryptionFormat::Luks2,
            ("aes", "xts-plain64"),
            None,
            volume_key,
            match integrity {
                IntegrityType::None => None,
                IntegrityType::Journal | IntegrityType::NoJournal => Some(&mut params_ref),
            },
        )?;
        device.keyslot_handle().add_by_key(
            None,
            Some(volume_key),
            passphrase.as_bytes(),
            CryptVolumeKey::empty(),
        )?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
    .with_context(|| format!("Failed to format {dev} as LUKS2 volume"))?;

    Ok(())
}

pub async fn check_passphrase(dev: &str, passphrase: &Passphrase) -> Result<(), anyhow::Error> {
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    let device_path = PathBuf::from(&dev);

    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let mut device = CryptInit::init(&device_path)?;

        device
            .context_handle()
            .load::<()>(Some(EncryptionFormat::Luks2), None)?;
        device.activate_handle().activate_by_passphrase(
            None,
            None,
            passphrase.as_bytes(),
            CryptActivate::empty(),
        )?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
    .with_context(|| format!("Failed to check passphrase for device {dev}"))?;

    Ok(())
}

pub async fn open_with_check_passphrase(
    volume: &str,
    dev: &str,
    passphrase: &Passphrase,
    integrity: IntegrityType,
) -> Result<(), anyhow::Error> {
    let passphrase = passphrase.to_owned();
    let verbose = get_verbose().await;

    crate::fs::luks2::check_passphrase(dev, &passphrase)
        .await
        .with_context(||format!("Passphrase verification failed for volume {}: the passphrase is likely incorrect. Please check your passphrase configuration.", volume))?;

    let device_path = PathBuf::from(&dev);
    let volume_name = volume.to_owned();
    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let mut device = CryptInit::init(&device_path)?;

        device
            .context_handle()
            .load::<()>(Some(EncryptionFormat::Luks2), None)?;
        device.activate_handle().activate_by_passphrase(
            Some(&volume_name),
            None,
            passphrase.as_bytes(),
            match integrity {
                IntegrityType::None | IntegrityType::Journal => CryptActivate::empty(),
                IntegrityType::NoJournal => CryptActivate::empty() | CryptActivate::NO_JOURNAL,
            },
        )?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
    .with_context(|| format!("Failed to setup mapping for volume {volume}"))?;

    Ok(())
}

pub async fn is_initialized(dev: &str) -> Result<bool> {
    let verbose = get_verbose().await;
    let device_path = PathBuf::from(&dev);

    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let mut device = CryptInit::init(&device_path)?;

        let load_success = device.context_handle().load::<()>(None, None).is_ok();

        let is_luks2 =
            load_success && device.format_handle().get_type()? == EncryptionFormat::Luks2;

        Ok::<_, anyhow::Error>(is_luks2)
    })
    .await?
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
    let volume_name = volume.to_owned();

    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let mut device = CryptInit::init_by_name_and_header(&volume_name, None)?;
        device
            .activate_handle()
            .deactivate(&volume_name, CryptDeactivate::empty())?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
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
    tracing::info!(
        "Initializing {} fs on volume {}, with volume integrity type {:?}",
        makefs,
        volume,
        integrity
    );
    match integrity {
        IntegrityType::None => NormalMakeFs::mkfs(device_path, makefs).await,
        IntegrityType::Journal | IntegrityType::NoJournal => {
            IntegrityNoWipeMakeFs::mkfs(device_path, makefs).await
        }
    }
    .with_context(|| format!("Failed to initialize {makefs} fs on volume {volume}"))?;
    Ok(())
}
