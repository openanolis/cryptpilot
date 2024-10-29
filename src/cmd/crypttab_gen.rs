use anyhow::Result;

use crate::{cli::CrypttabGenOptions, socket::SOCK_ADDR_DEFAULT};

pub async fn cmd_crypttab_gen(options: CrypttabGenOptions) -> Result<()> {
    let volume_configs = crate::config::load_volume_configs().await?;

    let crypttab = volume_configs
        .iter()
        .map(|volume_config| {
            format!(
                "{}\t{}\t{}",
                volume_config.volume,
                volume_config.dev,
                options.socket.as_deref().unwrap_or(SOCK_ADDR_DEFAULT)
            )
        })
        .collect::<Vec<String>>()
        .join("\n");

    println!("{crypttab}");

    Ok(())
}
