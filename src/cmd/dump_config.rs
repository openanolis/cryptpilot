use anyhow::Result;

use crate::config::source::cloud_init::CLOUD_INIT_CONFIG_BUNDLE_HEADER;

pub async fn cmd_dump_config() -> Result<()> {
    let config = crate::config::source::get_config_source()
        .await
        .get_config()
        .await?;

    println!(
        "{CLOUD_INIT_CONFIG_BUNDLE_HEADER}\n\n{}",
        toml::to_string(&config)?
    );
    Ok(())
}
