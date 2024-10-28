use anyhow::Result;

const SOCK_ADDR: &str = "/tmp/cryptpilot.sock";

pub fn cmd_gen_crypttab() -> Result<()> {
    let volume_configs = crate::config::load_volume_configs()?;

    let crypttab = volume_configs
        .iter()
        .map(|volume_config| {
            format!(
                "{}\t{}\t{}",
                volume_config.volume, volume_config.dev, SOCK_ADDR
            )
        })
        .collect::<Vec<String>>()
        .join("\n");

    println!("{crypttab}");

    Ok(())
}
