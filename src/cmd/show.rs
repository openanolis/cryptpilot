use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;

use crate::cli::ShowOptions;
use crate::config::encrypt::KeyProviderConfig;
use crate::config::volume::VolumeConfig;

pub struct ShowCommand {
    pub show_options: ShowOptions,
}

#[async_trait]
impl super::Command for ShowCommand {
    async fn run(&self) -> Result<()> {
        let volume_configs = crate::config::source::get_config_source()
            .await
            .get_volume_configs()
            .await?;

        volume_configs.print_as_table().await?;

        Ok(())
    }
}

#[async_trait]
pub trait PrintAsTable {
    async fn print_as_table(&self) -> Result<()>;
}

#[async_trait]
impl PrintAsTable for VolumeConfig {
    async fn print_as_table(&self) -> Result<()> {
        std::slice::from_ref(self).print_as_table().await
    }
}

#[async_trait]
impl PrintAsTable for [VolumeConfig] {
    async fn print_as_table(&self) -> Result<()> {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                "Volume",
                "Volume Path",
                "Underlay Device",
                "Key Provider",
                "Extra Options",
                "Initialized",
                "Opened",
            ]);

        for volume_config in self {
            let dev_exist = Path::new(&volume_config.dev).exists();

            table.add_row(vec![
                Cell::new(volume_config.volume.as_str()),
                if !dev_exist {
                    Cell::new("N/A").fg(Color::Yellow)
                } else if crate::fs::luks2::is_active(&volume_config.volume) {
                    Cell::new(volume_config.volume_path().display()).fg(Color::Green)
                } else {
                    Cell::new("<not opened>").fg(Color::Yellow)
                },
                if dev_exist {
                    Cell::new(volume_config.dev.as_str())
                } else {
                    tracing::warn!("Device {} does not exist", volume_config.dev);
                    Cell::new(format!("{} <not exist>", volume_config.dev)).fg(Color::Red)
                },
                Cell::new(serde_variant::to_variant_name(
                    &volume_config.encrypt.key_provider,
                )?),
                {
                    let s = toml::to_string_pretty(&volume_config.extra_config)?;
                    if s.is_empty() {
                        Cell::new("<none>").fg(Color::DarkGrey)
                    } else {
                        Cell::new(s)
                    }
                },
                {
                    if !dev_exist {
                        Cell::new("N/A").fg(Color::Yellow)
                    } else if let KeyProviderConfig::Otp(_) = volume_config.encrypt.key_provider {
                        Cell::new("Not Required").fg(Color::Green)
                    } else {
                        match crate::fs::luks2::is_initialized(&volume_config.dev).await {
                            Ok(initialized) => {
                                if initialized {
                                    Cell::new("True").fg(Color::Green)
                                } else {
                                    Cell::new("False").fg(Color::Yellow)
                                }
                            }
                            Err(e) => {
                                tracing::warn!("{e:?}");
                                Cell::new("Error").fg(Color::Red)
                            }
                        }
                    }
                },
                if !dev_exist {
                    Cell::new("N/A").fg(Color::Yellow)
                } else if crate::fs::luks2::is_active(&volume_config.volume) {
                    Cell::new("True").fg(Color::Green)
                } else {
                    Cell::new("False").fg(Color::Yellow)
                },
            ]);
        }

        println!("{table}");

        Ok(())
    }
}
