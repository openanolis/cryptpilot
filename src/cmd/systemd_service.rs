use anyhow::Result;
use log::{error, info};

use crate::cli::SystemdServiceOptions;

pub async fn cmd_systemd_service(_systemd_service_options: &SystemdServiceOptions) -> Result<()> {
    info!("Loading volume configs");
    let volume_configs = crate::config::load_volume_configs().await?;
    super::show::print_volume_configs_as_table(&volume_configs).await?;

    info!("Opening volumes according to volume configs");
    for volume_config in &volume_configs {
        info!(
            "Setting up mapping for volume {} from device {}",
            volume_config.volume, volume_config.dev
        );
        match super::open::open_for_specific_volume(&volume_config).await {
            Ok(_) => {
                info!(
                    "The mapping for volume {} is active now",
                    volume_config.volume
                );
            }
            Err(e) => {
                error!(
                    "Failed to setup mapping for volume {}: {e:#}",
                    volume_config.volume,
                )
            }
        };
    }

    info!("Checking status again");
    super::show::print_volume_configs_as_table(&volume_configs).await?;
    info!("Everything have been completed, exit now");

    Ok(())
}
