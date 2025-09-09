use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::{
    cli::ShowReferenceValueStage,
    cmd::fde::disk::MeasurementedBootComponents,
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
            Some(disk) => Box::new(OnExternalFdeDisk::new_from_disk(disk).await?),
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

        inseart_with_hash::<sha2::Sha384>(&boot_measurement, &mut map, "SHA384")?;
        inseart_with_hash::<sm3::Sm3>(&boot_measurement, &mut map, "SM3")?;

        map.insert(
            "kernel_cmdline".to_string(),
            vec![boot_measurement.kernel_cmdline],
        );

        let json = serde_json::to_string_pretty(&map)?;

        println!("{json:#}");

        Ok(())
    }
}

fn inseart_with_hash<T>(
    boot_measurement: &MeasurementedBootComponents,
    map: &mut IndexMap<String, Vec<String>>,
    hash_key: &str,
) -> Result<()>
where
    T: digest::Digest + digest::Update,
{
    let hash_value = boot_measurement.cal_hash::<T>()?;

    map.insert(
        format!("measurement.kernel_cmdline.{hash_key}"),
        vec![hash_value.kernel_cmdline_hash],
    );

    map.insert(
        format!("measurement.kernel.{hash_key}"),
        vec![hash_value.kernel_hash],
    );

    map.insert(
        format!("measurement.initrd.{hash_key}"),
        vec![hash_value.initrd_hash],
    );

    map.insert(
        format!("measurement.grub.{hash_key}"),
        hash_value.grub_authenticode_hashes,
    );

    map.insert(
        format!("measurement.shim.{hash_key}"),
        hash_value.shim_authenticode_hashes,
    );

    Ok(())
}
