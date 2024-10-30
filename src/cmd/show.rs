use anyhow::Result;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;

use crate::config::{KeyProviderOptions, VolumeConfig};
use crate::luks2;

pub async fn cmd_show() -> Result<()> {
    let volume_configs = crate::config::load_volume_configs().await?;

    print_volume_configs_as_table(&volume_configs).await?;

    Ok(())
}

pub async fn print_volume_configs_as_table(volume_configs: &[VolumeConfig]) -> Result<()> {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "Volume",
            "Device",
            "Key Provider Type",
            "Extra Options",
            "Initialized",
            "Opened",
        ]);

    for volume_config in volume_configs {
        table.add_row(vec![
            Cell::new(volume_config.volume.as_str()),
            Cell::new(volume_config.dev.as_str()),
            Cell::new(serde_variant::to_variant_name(&volume_config.key_provider)?),
            {
                let s = toml::to_string_pretty(&volume_config.extra_options)?;
                if s.is_empty() {
                    Cell::new("<none>").fg(Color::DarkGrey)
                } else {
                    Cell::new(s)
                }
            },
            {
                if let KeyProviderOptions::Otp(_) = volume_config.key_provider {
                    Cell::new("Not Required").fg(Color::Green)
                } else {
                    if crate::luks2::is_initialized(&volume_config.dev).await? {
                        Cell::new("True").fg(Color::Green)
                    } else {
                        Cell::new("False").fg(Color::Yellow)
                    }
                }
            },
            if luks2::is_active(&volume_config.volume) {
                Cell::new("True").fg(Color::Green)
            } else {
                Cell::new("False").fg(Color::Yellow)
            },
        ]);
    }

    println!("{table}");

    Ok(())
}
