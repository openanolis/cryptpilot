use anyhow::Result;

use crate::cli::BootServiceOptions;

use crate::cmd::show::PrintAsTable;

pub async fn setup_user_provided_volumes(_boot_service_options: &BootServiceOptions) -> Result<()> {
    tracing::info!("Checking status for all volumes now");
    let volume_configs = crate::config::get_volume_config_source()
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
        // We only open volumes with auto_open=true
        if volume_config.extra_config.auto_open != Some(true) {
            tracing::info!(
                "Volume {} is skipped since 'auto_open' is not explicitly set to true",
                volume_config.volume
            );
            continue;
        }

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
