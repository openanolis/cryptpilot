use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use libcryptsetup_rs::{
    consts::{
        flags::{CryptActivate, CryptDeactivate, CryptVolumeKey},
        vals::{CryptDebugLevel, EncryptionFormat},
    },
    CryptInit, CryptParamsLuks2, CryptParamsLuks2Ref,
};
use rand::{distributions::Alphanumeric, Rng as _};
use tokio::fs::OpenOptions;
use tokio::io::AsyncReadExt;

use crate::types::{IntegrityType, Passphrase};

use super::get_verbose;

const LUKS2_VOLUME_KEY_SIZE_BIT_WITH_INTEGRITY: usize = 768;
const LUKS2_VOLUME_KEY_SIZE_BIT_WITHOUT_INTEGRITY: usize = 512;
const LUKS2_SECTOR_SIZE: u32 = 4096;
const LUKS2_SUBSYSTEM_NAME: &str = "cryptpilot";

async fn get_luks2_subsystem(dev: &Path) -> Result<Option<String>> {
    /// LUKS2 header structure according to the specification
    /// Reference: https://gitlab.com/cryptsetup/cryptsetup/-/blob/24d10f412e2ca1b0a8ed5addb1381507662a9862/lib/luks2/luks2.h
    #[repr(C, packed)]
    #[derive(Debug, Clone)]
    struct Luks2HdrDisk {
        magic: [u8; 6],
        version: u16, // big endian
        hdr_size: u64,
        seqid: u64,
        label: [u8; 48],
        checksum_alg: [u8; 32],
        salt: [u8; 64],
        uuid: [u8; 40],
        subsystem: [u8; 48],
        hdr_offset: u64,
        padding: [u8; 184],
        csum: [u8; 64],
        padding4096: [u8; 7 * 512],
    }

    let mut file = tokio::fs::File::open(dev).await?;

    let mut header_buf = vec![0u8; std::mem::size_of::<Luks2HdrDisk>()];
    file.read_exact(&mut header_buf).await?;
    if header_buf.len() < std::mem::size_of::<Luks2HdrDisk>() {
        return Err(anyhow::anyhow!(
            "Header buffer size is too small: {} bytes, expected at least {} bytes",
            header_buf.len(),
            std::mem::size_of::<Luks2HdrDisk>()
        ));
    }

    let header: &Luks2HdrDisk = unsafe { &*(header_buf.as_ptr() as *const Luks2HdrDisk) };

    let magic_bytes = &header.magic;
    if !(magic_bytes == b"LUKS\xba\xbe" || magic_bytes == b"SKUL\xba\xbe")
        || u16::from_be(header.version) != 2
    {
        return Err(anyhow::anyhow!(
            "Invalid LUKS2 header: magic='{:?}', version={}",
            magic_bytes,
            u16::from_be(header.version)
        ));
    }

    let subsystem_str = match header.subsystem.iter().position(|&x| x == 0) {
        Some(pos) => String::from_utf8_lossy(&header.subsystem[..pos]).to_string(),
        None => String::from_utf8_lossy(&header.subsystem).to_string(),
    };

    if !subsystem_str.is_empty() && subsystem_str != "-" {
        tracing::debug!("Found LUKS2 subsystem in binary header: {}", subsystem_str);
        Ok(Some(subsystem_str))
    } else {
        Ok(None)
    }
}

pub async fn format(dev: &Path, passphrase: &Passphrase, integrity: IntegrityType) -> Result<()> {
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
    .with_context(|| format!("Failed to format {dev:?} as LUKS2 volume"))?;

    Ok(())
}

pub async fn mark_volume_as_initialized(dev: &Path) -> Result<()> {
    let verbose = get_verbose().await;
    let dev_path = dev.to_path_buf();
    let dev_path_for_error = dev_path.clone();

    tokio::task::spawn_blocking(move || {
        if verbose {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::All);
        } else {
            libcryptsetup_rs::set_debug_level(CryptDebugLevel::None);
        }

        let mut device = CryptInit::init(&dev_path)?;

        // Load the device
        device
            .context_handle()
            .load::<()>(Some(EncryptionFormat::Luks2), None)?;

        // Mark the volume as initialized by setting the subsystem to "cryptpilot"
        device
            .context_handle()
            .set_label(None, Some(LUKS2_SUBSYSTEM_NAME))?;

        Ok::<_, anyhow::Error>(())
    })
    .await?
    .with_context(|| {
        format!(
            "Failed to mark volume as initialized for {:?}",
            dev_path_for_error
        )
    })?;

    Ok(())
}

pub async fn check_passphrase(dev: &Path, passphrase: &Passphrase) -> Result<(), anyhow::Error> {
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
    .with_context(|| format!("Failed to check passphrase for device {dev:?}"))?;

    Ok(())
}

pub async fn open_with_check_passphrase(
    volume: &str,
    dev: &Path,
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

pub async fn is_initialized(dev: &Path) -> Result<bool> {
    is_a_cryptpilot_initialized_luks2_volume(dev).await
}

async fn is_a_cryptpilot_initialized_luks2_volume(dev: &Path) -> Result<bool> {
    let verbose = get_verbose().await;
    let device_path = PathBuf::from(&dev);

    // First check if it's a LUKS2 volume using the blocking operation
    let is_luks2 = tokio::task::spawn_blocking(move || {
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
    .with_context(|| format!("Failed to check luks2 initialization status of device {dev:?}"))?;

    if !is_luks2 {
        return Ok(false);
    }

    // Check if the subsystem is set to "cryptpilot"
    let subsystem_is_set = get_luks2_subsystem(dev)
        .await
        .with_context(|| format!("Failed to get LUKS2 device subsystem for {dev:?}"))?
        .map(|subsystem| subsystem == LUKS2_SUBSYSTEM_NAME)
        .unwrap_or(false);

    Ok(subsystem_is_set)
}

pub fn is_active(volume: &str) -> bool {
    PathBuf::from(format!("/dev/mapper/{}", volume)).exists()
}

pub async fn is_dev_in_use(dev: &Path) -> Result<bool> {
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

pub struct TempLuksVolume(String);

impl TempLuksVolume {
    pub async fn open(
        dev: &Path,
        passphrase: &Passphrase,
        integrity: IntegrityType,
    ) -> Result<Self> {
        let name = format!(
            ".cryptpilot-{}",
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect::<String>()
        );
        tracing::info!("Setting up a temporary luks volume {name}",);
        crate::fs::luks2::open_with_check_passphrase(&name, dev, passphrase, integrity).await?;
        Ok(Self(name))
    }

    pub fn volume_path(&self) -> PathBuf {
        Path::new("/dev/mapper").join(&self.0)
    }
}

impl Drop for TempLuksVolume {
    fn drop(&mut self) {
        let name = self.0.clone();
        crate::async_defer!(async {
            tracing::info!("Closing the temporary luks volume {name}");
            let _ = crate::fs::luks2::close(&name).await;
        });
    }
}
