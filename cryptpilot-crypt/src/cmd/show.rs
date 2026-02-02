use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::*;
use serde::Serialize;

use crate::cli::ShowOptions;
use cryptpilot::provider::{IntoProvider, KeyProvider as _, VolumeType};

use crate::config::VolumeConfig;

pub struct ShowCommand {
    #[allow(dead_code)]
    pub show_options: ShowOptions,
}

#[async_trait]
impl crate::cmd::Command for ShowCommand {
    async fn run(&self) -> Result<()> {
        let mut volume_configs = crate::config::get_volume_config_source()
            .await
            .get_volume_configs()
            .await?;

        // Filter volumes if specific names are provided
        if !self.show_options.volume.is_empty() {
            volume_configs.retain(|config| self.show_options.volume.contains(&config.volume));
        }

        if self.show_options.json {
            volume_configs.print_as_json().await?;
        } else {
            volume_configs.print_as_table().await?;
        }

        Ok(())
    }
}

#[async_trait]
pub trait PrintAsTable {
    async fn print_as_table(&self) -> Result<()>;
}

#[async_trait]
pub trait PrintAsJson {
    async fn print_as_json(&self) -> Result<()>;
}

/// JSON serializable structure for volume status
#[derive(Serialize)]
struct VolumeStatus {
    volume: String,
    volume_path: PathBuf,
    underlay_device: PathBuf,
    device_exists: bool,
    key_provider: String,
    key_provider_options: serde_json::Value,
    extra_options: serde_json::Value,
    needs_initialize: bool,
    initialized: bool,
    opened: bool,
}

impl VolumeStatus {
    /// Build volume status from config
    async fn from_config(volume_config: &VolumeConfig) -> Result<Self> {
        let dev_exist = Path::new(&volume_config.dev).exists();
        let is_open = cryptpilot::fs::luks2::is_active(&volume_config.volume);

        let volume_path = volume_config.volume_path();

        let key_provider = serde_variant::to_variant_name(&volume_config.encrypt.key_provider)
            .unwrap_or("unknown")
            .to_string();

        let key_provider_options = serde_json::to_value(&volume_config.encrypt.key_provider)
            .unwrap_or(serde_json::Value::Null);

        let extra_options = match serde_json::to_value(&volume_config.extra_config) {
            Ok(v) => v,
            Err(_) => serde_json::Value::Null,
        };

        let needs_initialize = matches!(
            volume_config
                .encrypt
                .key_provider
                .clone()
                .into_provider()
                .volume_type(),
            VolumeType::Persistent
        );

        let initialized = if !dev_exist {
            false
        } else if !needs_initialize {
            true
        } else {
            match cryptpilot::fs::luks2::is_initialized(&volume_config.dev).await {
                Ok(initialized) => initialized,
                Err(e) => {
                    tracing::warn!("Failed to check initialization status: {e:?}");
                    false
                }
            }
        };

        Ok(Self {
            volume: volume_config.volume.clone(),
            volume_path,
            underlay_device: volume_config.dev.clone(),
            device_exists: dev_exist,
            key_provider,
            key_provider_options,
            extra_options,
            needs_initialize,
            initialized,
            opened: is_open,
        })
    }
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
            let status = VolumeStatus::from_config(volume_config).await?;

            table.add_row(vec![
                Cell::new(&status.volume),
                if !status.device_exists {
                    Cell::new("N/A").fg(Color::Yellow)
                } else if status.opened {
                    Cell::new(status.volume_path.to_string_lossy()).fg(Color::Green)
                } else {
                    Cell::new("<not opened>").fg(Color::Yellow)
                },
                if status.device_exists {
                    Cell::new(status.underlay_device.to_string_lossy())
                } else {
                    tracing::warn!("Device {:?} does not exist", status.underlay_device);
                    Cell::new(format!("{:?} <not exist>", status.underlay_device)).fg(Color::Red)
                },
                Cell::new(&status.key_provider),
                {
                    if status.extra_options.is_null()
                        || status.extra_options == serde_json::json!({})
                    {
                        Cell::new("<none>").fg(Color::DarkGrey)
                    } else {
                        let s = toml::to_string_pretty(&volume_config.extra_config)?;
                        Cell::new(s)
                    }
                },
                if !status.needs_initialize {
                    Cell::new("Not Required").fg(Color::Yellow)
                } else if !status.device_exists {
                    Cell::new("N/A").fg(Color::Yellow)
                } else if status.initialized {
                    Cell::new("True").fg(Color::Green)
                } else {
                    Cell::new("False").fg(Color::Yellow)
                },
                if status.opened {
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

#[async_trait]
impl PrintAsJson for VolumeConfig {
    async fn print_as_json(&self) -> Result<()> {
        std::slice::from_ref(self).print_as_json().await
    }
}

#[async_trait]
impl PrintAsJson for [VolumeConfig] {
    async fn print_as_json(&self) -> Result<()> {
        let mut statuses = Vec::new();

        for volume_config in self {
            statuses.push(VolumeStatus::from_config(volume_config).await?);
        }

        let json = serde_json::to_string_pretty(&statuses)?;
        println!("{}", json);

        Ok(())
    }
}
