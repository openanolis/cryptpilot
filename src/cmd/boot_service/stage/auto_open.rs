use anyhow::Result;

use crate::{
    cli::{BootServiceOptions, BootStage},
    cmd::show::PrintAsTable,
};

pub async fn setup_user_provided_volumes(boot_service_options: &BootServiceOptions) -> Result<()> {
    tracing::info!("Checking status for all volumes now");
    let volume_configs = crate::config::source::get_config_source()
        .await
        .get_volume_configs()
        .await?;
    if volume_configs.is_empty() {
        tracing::info!("The volume configs is empty, exit now");
        return Ok(());
    }
    volume_configs.print_as_table().await?;
    tracing::info!("Opening volumes according to volume configs");
    for volume_config in &volume_configs {
        match boot_service_options.stage {
            BootStage::InitrdFdeBeforeSysroot
                if volume_config.extra_config.auto_open != Some(true) =>
            {
                tracing::info!(
                    "Volume {} is skipped since 'auto_open = false'",
                    volume_config.volume
                );
                continue;
            }
            BootStage::InitrdFdeAfterSysroot => {
                unreachable!("This should never happen in initrd-fde-after-sysroot stage")
            }
            _ => { /* Accept */ }
        };

        tracing::info!(
            "Setting up mapping for volume {} from device {}",
            volume_config.volume,
            volume_config.dev
        );
        match crate::cmd::open::open_for_specific_volume(volume_config).await {
            Ok(_) => {
                tracing::info!(
                    "The mapping for volume {} is active now",
                    volume_config.volume
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to setup mapping for volume {}: {e:?}",
                    volume_config.volume,
                )
            }
        };
    }
    tracing::info!("Checking status for all volumes again");
    volume_configs.print_as_table().await?;
    Ok(())
}
