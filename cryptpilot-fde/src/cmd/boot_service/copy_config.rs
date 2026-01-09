use anyhow::{bail, Context, Result};

use crate::{
    cmd::boot_service::initrd_state::InitrdState,
    config::{
        cloud_init::CloudInitConfigSource, fs::FileSystemConfigSource,
        initrd_state::InitrdStateConfigSource, FdeConfigBundle, FdeConfigSource,
    },
};
use cryptpilot::measure::{AutoDetectMeasure, Measure, OPERATION_NAME_LOAD_CONFIG_UNTRUSTED};

pub async fn copy_config_to_initrd_state_if_not_exist(
    measurement_if_from_unsafe_source: bool,
) -> Result<()> {
    if InitrdStateConfigSource::exist() {
        return Ok(());
    }

    let fde_config_bundle = load_fde_config_bundle(measurement_if_from_unsafe_source).await?;

    // Save to initrd state
    let initrd_state = InitrdState { fde_config_bundle };
    initrd_state.save().await?;

    Ok(())
}

async fn load_fde_config_bundle(
    measurement_if_from_unsafe_source: bool,
) -> Result<FdeConfigBundle> {
    tracing::info!("Trying to load config from cloud-init");
    match load_config_from_cloud_init().await {
        Ok(config) => {
            if measurement_if_from_unsafe_source {
                // Extend config hash to runtime measurement
                let content_to_hash = config.gen_hash_content()?;

                let measure = AutoDetectMeasure::new().await;
                match measure
                    .extend_measurement_hash(OPERATION_NAME_LOAD_CONFIG_UNTRUSTED.into(), content_to_hash)
                    .await
                    .context("Using cryptpilot config from untrusted source (cloud-init), but failed to measure it") {
                    Ok(()) => {
                        return Ok(config);
                    },
                    // Will not use this config if measurement failed
                    Err(e) => tracing::warn!("{e:?}"),
                }
            } else {
                return Ok(config);
            }
        }
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
    FileSystemConfigSource::new_with_default_config_dir()
        .get_fde_config_bundle()
        .await
}

async fn load_config_from_cloud_init() -> Result<FdeConfigBundle> {
    CloudInitConfigSource::new().get_fde_config_bundle().await
}
