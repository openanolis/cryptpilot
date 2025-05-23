use anyhow::{anyhow, bail, Context, Result};
use log::{info, warn};

use crate::{
    cmd::boot_service::{
        detect_root_part,
        initrd_state::{serialize_initrd_state, InitrdState},
    },
    config::{
        source::{cloud_init::CloudInitConfigSource, fs::FileSystemConfigSource, ConfigSource},
        ConfigBundle,
    },
    fs::mount::TmpMountPoint,
    measure::{AutoDetectMeasure, Measure, OPERATION_NAME_LOAD_CONFIG},
};

use super::{detect_boot_part, initrd_state::InitrdStateConfigSource};

const CRYPTPILOT_CONFIG_DIR_INITRD_UNTRUSTED: &'static str = "cryptpilot/config";

pub async fn copy_config_to_initrd_state_if_not_exist(extend_measurement: bool) -> Result<()> {
    if InitrdStateConfigSource::exist() {
        return Ok(());
    }

    let config = load_config().await?;
    let config_str = toml::to_string(&config)?;

    // Save to initrd state
    let initrd_state = InitrdState { config };
    serialize_initrd_state(&initrd_state).await?;

    if extend_measurement {
        // Extend config hash to runtime measurement
        let measure = AutoDetectMeasure::new().await;
        if let Err(e) = measure
            .extend_measurement_hash(OPERATION_NAME_LOAD_CONFIG.into(), config_str)
            .await
            .context("Failed to extend cryptpilot config hash to runtime measurement")
        {
            warn!("{e:?}")
        }
    }

    Ok(())
}

async fn load_config() -> Result<ConfigBundle> {
    info!("Trying to load config from cloud-init");
    match load_config_from_cloud_init().await {
        Ok(config) => return Ok(config),
        Err(e) => {
            info!("Failed to load config from cloud-init: {e:?}");
        }
    };

    info!("Trying to load config from from partition");
    match load_config_from_boot_dir().await {
        Ok(config) => return Ok(config),
        Err(e) => {
            info!("Failed to load config from partition: {e:?}");
        }
    };

    bail!("Failed to load config from any source");
}

async fn load_config_from_boot_dir_in_boot_part_callback(
    mount_point: std::path::PathBuf,
) -> Result<ConfigBundle> {
    let config_dir = mount_point.join(CRYPTPILOT_CONFIG_DIR_INITRD_UNTRUSTED);
    if !config_dir.exists() {
        bail!(
            "Can not find config dir ({CRYPTPILOT_CONFIG_DIR_INITRD_UNTRUSTED}) in boot partition"
        )
    }
    FileSystemConfigSource::new(config_dir).get_config().await
}

async fn load_config_from_boot_dir_in_root_part_callback(
    mount_point: std::path::PathBuf,
) -> Result<ConfigBundle> {
    let config_dir = mount_point
        .join("boot")
        .join(CRYPTPILOT_CONFIG_DIR_INITRD_UNTRUSTED);
    if !config_dir.exists() {
        bail!("Can not find config dir (/boot/{CRYPTPILOT_CONFIG_DIR_INITRD_UNTRUSTED}) in root partition")
    }
    FileSystemConfigSource::new(config_dir).get_config().await
}

async fn load_config_from_boot_dir() -> Result<ConfigBundle> {
    let e = match detect_boot_part().await {
        Ok(boot_part) => {
            match TmpMountPoint::with_new_mount(
                &boot_part,
                load_config_from_boot_dir_in_boot_part_callback,
            )
            .await
            .and_then(|e| e)
            {
                Ok(config) => return Ok(config),
                Err(e) => e,
            }
        }
        Err(_) => anyhow!("Failed to detect boot partition"),
    };

    info!("Failed to load config from boot partition, try to load from root partition: {e:?}");

    let e = match detect_root_part().await {
        Ok(root_part) => {
            match TmpMountPoint::with_new_mount(
                &root_part,
                load_config_from_boot_dir_in_root_part_callback,
            )
            .await
            .and_then(|e| e)
            {
                Ok(config) => return Ok(config),
                Err(e) => e,
            }
        }
        Err(_) => anyhow!("Failed to detect root partition"),
    };

    bail!("Failed to load config from both boot and root partition: {e:?}")
}

async fn load_config_from_cloud_init() -> Result<ConfigBundle> {
    CloudInitConfigSource::new().get_config().await
}
