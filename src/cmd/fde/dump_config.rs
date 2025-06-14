use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    cmd::fde::disk::{FdeDisk, OnCurrentSystemFdeDisk, OnExternalFdeDisk},
    config::source::cloud_init::CLOUD_INIT_FDE_CONFIG_BUNDLE_HEADER,
};

pub struct ConfigDumpCommand {
    pub disk: Option<PathBuf>,
}

#[async_trait]
impl super::super::Command for ConfigDumpCommand {
    async fn run(&self) -> Result<()> {
        let fde_disk: Box<dyn FdeDisk + Send> = match &self.disk {
            Some(disk) => Box::new(OnExternalFdeDisk::new_from_disk(&disk).await?),
            None => Box::new(OnCurrentSystemFdeDisk::new().await?),
        };

        let fde_config_bundle = fde_disk.load_fde_config_bundle().await?;

        let hash_hex = fde_config_bundle.gen_hash_hex()?;
        let hash_content_pretty = fde_config_bundle.gen_hash_content_pretty()?;

        println!(
            r#"{CLOUD_INIT_FDE_CONFIG_BUNDLE_HEADER}

# This config is generated by cryptpilot. And you can also put this cloud-init user data of your instance
#
# The sha384 hash of this config is: {hash_hex}


{hash_content_pretty}"#
        );
        Ok(())
    }
}
