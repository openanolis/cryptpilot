use anyhow::Result;
use log::info;

use crate::cli::CloseOptions;

pub async fn cmd_close(close_options: &CloseOptions) -> Result<()> {
    let volume = close_options.volume.to_owned();

    if !crate::fs::luks2::is_active(&volume) {
        info!("The mapping for {} is not active, nothing to do", volume);
        return Ok(());
    }

    info!("Removing mapping for {volume}");
    crate::fs::luks2::close(&volume).await?;
    info!("The mapping is removed now");

    Ok(())
}
