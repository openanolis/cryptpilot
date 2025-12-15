use anyhow::{bail, Context, Result};

use crate::{
    cmd::boot_service::initrd_state::InitrdState,
    config::source::{
        cloud_init::{CloudInitConfigSource, FdeConfigBundle},
        fs::FileSystemConfigSource,
        initrd_state::InitrdStateConfigSource,
        ConfigSource,
    },
    measure::{AutoDetectMeasure, Measure, OPERATION_NAME_LOAD_CONFIG},
};

pub async fn copy_config_to_initrd_state_if_not_exist(extend_measurement: bool) -> Result<()> {
    if InitrdStateConfigSource::exist() {
        return Ok(());
    }

    let fde_config_bundle = load_fde_config_bundle().await?;
    let content_to_hash = fde_config_bundle.gen_hash_content()?;

    // Save to initrd state
    let initrd_state = InitrdState { fde_config_bundle };
    initrd_state.save().await?;

    if extend_measurement {
        // Extend config hash to runtime measurement
        let measure = AutoDetectMeasure::new().await;
        if let Err(e) = measure
            .extend_measurement_hash(OPERATION_NAME_LOAD_CONFIG.into(), content_to_hash)
            .await
            .context("Failed to extend cryptpilot config hash to runtime measurement")
        {
            tracing::warn!("{e:?}")
        }
    }

    Ok(())
}

async fn load_fde_config_bundle() -> Result<FdeConfigBundle> {
    tracing::info!("Trying to load config from cloud-init");
    match load_config_from_cloud_init().await {
        Ok(config) => return Ok(config),
        Err(e) => {
            tracing::info!("Failed to load config from cloud-init: {e:?}");
        }
    };

    tracing::info!("Trying to load config from current initrd environment");
    match load_config_from_current_initrd_environment().await {
        Ok(config) => return Ok(config),
        Err(e) => {
            tracing::info!("Failed to load config from partition: {e:?}");
        }
    };

    bail!("Failed to load config from any source");
}

async fn load_config_from_current_initrd_environment() -> Result<FdeConfigBundle> {
    Ok(FileSystemConfigSource::new_with_default_config_dir()
        .get_config()
        .await?
        .strip_as_fde_config_bundle())
}

async fn load_config_from_cloud_init() -> Result<FdeConfigBundle> {
    Ok(CloudInitConfigSource::new()
        .get_config()
        .await?
        .strip_as_fde_config_bundle())
}
