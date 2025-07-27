use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::{
    cli::ShowReferenceValueStage,
    measure::{
        attestation_agent::AAEL_DOMAIN, OPERATION_NAME_FDE_ROOTFS_HASH,
        OPERATION_NAME_INITRD_SWITCH_ROOT, OPERATION_NAME_LOAD_CONFIG,
    },
};

use super::disk::{FdeDisk, OnCurrentSystemFdeDisk, OnExternalFdeDisk};

pub struct ShowReferenceValueCommand {
    pub disk: Option<PathBuf>,
    pub stage: ShowReferenceValueStage,
}

#[async_trait]
impl super::super::Command for ShowReferenceValueCommand {
    async fn run(&self) -> Result<()> {
        tracing::debug!("Get rootfs reference value");
        let fde_disk: Box<dyn FdeDisk + Send + Sync> = match &self.disk {
            Some(disk) => Box::new(OnExternalFdeDisk::new_from_disk(&disk).await?),
            None => Box::new(OnCurrentSystemFdeDisk::new().await?),
        };

        let fde_config_bundle = fde_disk.load_fde_config_bundle().await?;
        let hash_hex = fde_config_bundle.gen_hash_hex()?;

        let metadata = fde_disk.load_metadata().await?;
        let root_hash = metadata.root_hash;

        let mut map = IndexMap::new();
        map.insert(
            format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_LOAD_CONFIG}"),
            vec![hash_hex],
        );
        map.insert(
            format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_FDE_ROOTFS_HASH}"),
            vec![root_hash],
        );

        if matches!(self.stage, ShowReferenceValueStage::System) {
            map.insert(
                format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_INITRD_SWITCH_ROOT}"),
                vec!["{}".to_string()],
            );
        }

        tracing::debug!("Getting boot related measurement");
        let boot_measurement = fde_disk.get_boot_measurement().await?;

        map.insert(
            "kernel_cmdline".to_string(),
            vec![boot_measurement.kernel_cmdline],
        );

        map.insert(
            "measurement.kernel_cmdline.SHA384".to_string(),
            vec![boot_measurement.kernel_cmdline_sha384],
        );

        map.insert(
            "measurement.kernel.SHA384".to_string(),
            vec![boot_measurement.kernel_sha384],
        );

        map.insert(
            "measurement.initrd.SHA384".to_string(),
            vec![boot_measurement.initrd_sha384],
        );

        map.insert(
            "measurement.grub.SHA384".to_string(),
            vec![boot_measurement.grub_authenticode_sha384],
        );

        map.insert(
            "measurement.shim.SHA384".to_string(),
            vec![boot_measurement.shim_authenticode_sha384],
        );

        let json = serde_json::to_string_pretty(&map)?;

        println!("{json:#}");

        Ok(())
    }
}
