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

/// Unified volume status enumeration
#[derive(Serialize, Clone, Debug, PartialEq)]
pub enum VolumeStatusKind {
    /// Device does not exist physically
    DeviceNotFound,
    /// Device exists but initialization check failed (with error details)
    CheckFailed,
    /// Device requires initialization
    RequiresInit,
    /// Volume is ready to open (either initialized persistent volume or temporary volume)
    ReadyToOpen,
    /// Volume is currently opened/mapped
    Opened,
}

/// Detailed status information with human-readable description
#[derive(Serialize, Clone, Debug)]
pub struct VolumeStatus {
    /// Machine-readable status code
    #[serde(rename = "status")]
    pub kind: VolumeStatusKind,
    /// Human-readable detailed description
    pub description: String,
}

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

/// Individual volume status information
#[derive(Serialize)]
pub struct ShowVolume {
    volume: String,
    volume_path: PathBuf,
    underlay_device: PathBuf,
    key_provider: String,
    key_provider_options: serde_json::Value,
    extra_options: serde_json::Value,
    /// Unified status representation (flattened)
    #[serde(flatten)]
    status: VolumeStatus,
}

/// Collection of volume statuses for JSON output
#[derive(Serialize)]
struct VolumesCollection {
    volumes: Vec<ShowVolume>,
}

impl ShowVolume {
    /// Build volume status from config
    async fn from_config(volume_config: &VolumeConfig) -> Self {
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

        // Determine unified status using VolumeConfig method
        let status = volume_config.determine_status().await;

        Self {
            volume: volume_config.volume.clone(),
            volume_path,
            underlay_device: volume_config.dev.clone(),
            key_provider,
            key_provider_options,
            extra_options,
            status,
        }
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
                "Status",
            ]);

        for volume_config in self {
            let show_volume = ShowVolume::from_config(volume_config).await;

            // Determine color based on status code
            let status_color = match show_volume.status.kind {
                VolumeStatusKind::Opened => Color::Green,
                VolumeStatusKind::ReadyToOpen => Color::Green,
                VolumeStatusKind::RequiresInit => Color::Yellow,
                VolumeStatusKind::CheckFailed => Color::Red,
                VolumeStatusKind::DeviceNotFound => Color::Red,
            };

            table.add_row(vec![
                Cell::new(&show_volume.volume),
                match show_volume.status.kind {
                    VolumeStatusKind::DeviceNotFound => Cell::new("N/A").fg(Color::Yellow),
                    VolumeStatusKind::Opened => {
                        Cell::new(show_volume.volume_path.to_string_lossy().as_ref())
                            .fg(Color::Green)
                    }
                    _ => Cell::new("<not opened>").fg(Color::Yellow),
                },
                match show_volume.status.kind {
                    VolumeStatusKind::DeviceNotFound => {
                        tracing::warn!("Device {:?} does not exist", show_volume.underlay_device);
                        Cell::new(format!("{:?} <not exist>", show_volume.underlay_device))
                            .fg(Color::Red)
                    }
                    _ => Cell::new(show_volume.underlay_device.to_string_lossy().as_ref()),
                },
                Cell::new(&show_volume.key_provider),
                {
                    if show_volume.extra_options.is_null()
                        || show_volume.extra_options == serde_json::json!({})
                    {
                        Cell::new("<none>").fg(Color::DarkGrey)
                    } else {
                        let s = toml::to_string_pretty(&volume_config.extra_config)?;
                        Cell::new(s)
                    }
                },
                Cell::new(format!("{:?}", show_volume.status.kind)).fg(status_color),
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
        let mut volumes = Vec::new();

        for volume_config in self {
            volumes.push(ShowVolume::from_config(volume_config).await);
        }

        let volumes_collection = VolumesCollection { volumes };
        let json = serde_json::to_string_pretty(&volumes_collection)?;
        println!("{}", json);

        Ok(())
    }
}

// Implementation for VolumeConfig to determine status
impl VolumeConfig {
    /// Determine unified volume status with detailed description
    pub async fn determine_status(&self) -> VolumeStatus {
        // Check if volume is already opened
        let is_open = cryptpilot::fs::luks2::is_active(&self.volume);
        if is_open {
            return VolumeStatus {
                kind: VolumeStatusKind::Opened,
                description: format!(
                    "Volume '{}' is currently opened and mapped at '{}'",
                    self.volume,
                    self.volume_path().display()
                ),
            };
        }

        // Check if device exists
        let dev_exist = Path::new(&self.dev).exists();
        if !dev_exist {
            return VolumeStatus {
                kind: VolumeStatusKind::DeviceNotFound,
                description: format!("Device '{:?}' does not exist on filesystem", self.dev),
            };
        }

        // Check volume type
        let key_provider = self.encrypt.key_provider.clone().into_provider();
        let is_persistent = matches!(key_provider.volume_type(), VolumeType::Persistent);

        // For temporary volumes, they are ready to open without initialization
        if !is_persistent {
            return VolumeStatus {
                kind: VolumeStatusKind::ReadyToOpen,
                description: format!(
                    "Volume '{}' uses {} key provider (temporary volume) and is ready to open",
                    self.volume,
                    serde_variant::to_variant_name(&self.encrypt.key_provider).unwrap_or("unknown")
                ),
            };
        }

        // For persistent volumes, check initialization status
        match cryptpilot::fs::luks2::is_initialized(&self.dev).await {
            Ok(true) => VolumeStatus {
                kind: VolumeStatusKind::ReadyToOpen,
                description: format!(
                    "Device '{:?}' is properly initialized as LUKS2 volume and ready to open",
                    self.dev
                ),
            },
            Ok(false) => VolumeStatus {
                kind: VolumeStatusKind::RequiresInit,
                description: format!(
                    "Device '{:?}' exists but is not a valid LUKS2 volume - needs initialization",
                    self.dev
                ),
            },
            Err(e) => {
                // This is the critical case - check failed
                let error_msg = format!("{:?}", e);
                VolumeStatus {
                    kind: VolumeStatusKind::CheckFailed,
                    description: format!(
                        "Failed to check initialization status for device '{:?}': {}",
                        self.dev, error_msg
                    ),
                }
            }
        }
    }
}
