use std::path::PathBuf;

use anyhow::{bail, Result};
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::{
    cli::{ShowReferenceValueHashAlgo, ShowReferenceValueStage},
    cmd::fde::disk::MeasurementedBootComponents,
    measure::{
        attestation_agent::AAEL_DOMAIN, OPERATION_NAME_FDE_ROOTFS_HASH,
        OPERATION_NAME_INITRD_SWITCH_ROOT, OPERATION_NAME_LOAD_CONFIG,
    },
};

use super::disk::{FdeDisk, OnCurrentSystemFdeDisk, OnExternalFdeDisk};

pub struct ShowReferenceValueCommand {
    pub disk: Option<PathBuf>,
    pub stage: Option<ShowReferenceValueStage>,
    pub hash_algos: Vec<ShowReferenceValueHashAlgo>,
}

#[async_trait]
impl super::super::Command for ShowReferenceValueCommand {
    async fn run(&self) -> Result<()> {
        if self.hash_algos.is_empty() {
            bail!("No hash algorithm specified");
        }

        tracing::debug!("Get rootfs reference value");
        let mut map = IndexMap::new();

        let fde_disk: Box<dyn FdeDisk + Send + Sync> = match &self.disk {
            Some(disk) => Box::new(OnExternalFdeDisk::new_from_disk(disk).await?),
            None => Box::new(OnCurrentSystemFdeDisk::new().await?),
        };

        {
            let aael_key = format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_LOAD_CONFIG}");
            match fde_disk.load_fde_config_bundle().await {
                Ok(fde_config_bundle) => {
                    let hash_hex = fde_config_bundle.gen_hash_hex()?;
                    map.insert(aael_key, vec![hash_hex]);
                }
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "Failed to load fde config bundle, skip \"{aael_key}\""
                    );
                }
            };
        }

        {
            let aael_key = format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_FDE_ROOTFS_HASH}");
            match fde_disk.load_metadata().await {
                Ok(metadata) => {
                    let root_hash = metadata.root_hash;
                    map.insert(aael_key, vec![root_hash]);
                }
                Err(error) => {
                    tracing::warn!(?error, "Failed to load metadata, skip \"{aael_key}\"");
                }
            };
        }

        if matches!(self.stage, Some(ShowReferenceValueStage::System)) {
            map.insert(
                format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_INITRD_SWITCH_ROOT}"),
                vec!["{}".to_string()],
            );
        }

        tracing::debug!("Getting boot related measurement");

        let boot_components = fde_disk.get_boot_components().await?;
        tracing::debug!("Starting to calculate reference values for boot components");

        for hash_algo in &self.hash_algos {
            match hash_algo {
                ShowReferenceValueHashAlgo::Sha1 => {
                    inseart_with_hash::<sha1::Sha1>(&boot_components, &mut map, "SHA-1")?
                }
                ShowReferenceValueHashAlgo::Sha256 => {
                    inseart_with_hash::<sha2::Sha256>(&boot_components, &mut map, "SHA-256")?
                }
                ShowReferenceValueHashAlgo::Sha384 => {
                    inseart_with_hash::<sha2::Sha384>(&boot_components, &mut map, "SHA-384")?
                }
                ShowReferenceValueHashAlgo::Sm3 => {
                    inseart_with_hash::<sm3::Sm3>(&boot_components, &mut map, "SM3")?
                }
            }
        }

        map.insert(
            "kernel_cmdline".to_string(),
            boot_components
                .0
                .iter()
                .flat_map(|(_, kernel_artifacts)| {
                    kernel_artifacts
                        .kernel_cmdlines
                        .iter()
                        .map(|cmdline| format!("grub_kernel_cmdline {}", cmdline))
                })
                .collect::<Vec<_>>(),
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
        hash_value.kernel_cmdline_hashs,
    );

    map.insert(
        format!("measurement.kernel.{hash_key}"),
        hash_value.kernel_hashs,
    );

    map.insert(
        format!("measurement.initrd.{hash_key}"),
        hash_value.initrd_hashs,
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
