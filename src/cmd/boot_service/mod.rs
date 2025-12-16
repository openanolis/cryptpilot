pub mod copy_config;
pub mod initrd_state;
pub mod metadata;
pub mod stage;
pub mod time_sync;

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::cli::{BootServiceOptions, BootStage};

pub struct BootServiceCommand {
    pub boot_service_options: BootServiceOptions,
}

#[async_trait]
impl super::Command for BootServiceCommand {
    async fn run(&self) -> Result<()> {
        match &self.boot_service_options.stage {
            BootStage::InitrdFdeBeforeSysroot => {
                time_sync::sync_time_to_system().await?;

                stage::before_sysroot::setup_volumes_required_by_fde()
                    .await
                    .context("Failed to setup volumes required by FDE")?;
            }
            BootStage::InitrdFdeAfterSysroot => {
                stage::after_sysroot::setup_mounts_required_by_fde()
                    .await
                    .context("Failed to setup mounts required by FDE")?;
            }
            BootStage::SystemVolumesAutoOpen => {
                stage::auto_open::setup_user_provided_volumes(&self.boot_service_options)
                    .await
                    .context("Failed to setup volumes user provided automatically")?;
            }
        }

        tracing::info!("Everything have been completed, exit now");

        Ok(())
    }
}
