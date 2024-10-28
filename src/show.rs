use anyhow::Result;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;

pub fn cmd_show() -> Result<()> {
    let volume_configs = crate::config::load_volume_configs()?;

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(80)
        .set_header(vec![
            "Volume",
            "Device",
            "Key Provider Type",
            "Key Provider Options",
        ]);

    volume_configs
        .iter()
        .try_for_each(|volume_config| -> Result<()> {
            table.add_row(vec![
                Cell::new(volume_config.volume.as_str()),
                Cell::new(volume_config.dev.as_str()),
                Cell::new(serde_variant::to_variant_name(&volume_config.key_provider)?),
                Cell::new(
                    toml::to_string_pretty(&volume_config.key_provider)?
                        .lines()
                        .skip(1)
                        .collect::<Vec<&str>>()
                        .join("\n"),
                ),
            ]);

            Ok(())
        })?;

    println!("{table}");
    Ok(())
}
