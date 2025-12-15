use std::path::PathBuf;

use anyhow::{bail, Result};
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::{
    cli::{ShowReferenceValueHashAlgo, ShowReferenceValueStage},
    cmd::fde::disk::{
        artifacts::inseart_reference_value, current::OnCurrentSystemFdeDisk,
        external::OnExternalFdeDisk,
    },
    measure::{attestation_agent::AAEL_DOMAIN, OPERATION_NAME_INITRD_SWITCH_ROOT},
};

use super::disk::FdeDisk;

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

        tracing::debug!("Collecting boot related artifacts");
        let mut map = IndexMap::new();

        let fde_disk: Box<dyn FdeDisk + Send + Sync> = match &self.disk {
            Some(disk) => Box::new(OnExternalFdeDisk::new_from_disk(disk).await?),
            None => Box::new(OnCurrentSystemFdeDisk::new().await?),
        };

        let boot_artifacts = fde_disk.get_boot_artifacts().await?;
        tracing::debug!("Starting to calculate reference values");

        for hash_algo in &self.hash_algos {
            match hash_algo {
                ShowReferenceValueHashAlgo::Sha1 => {
                    inseart_reference_value::<sha1::Sha1>(&boot_artifacts, &mut map, "SHA-1")
                        .await?
                }
                ShowReferenceValueHashAlgo::Sha256 => {
                    inseart_reference_value::<sha2::Sha256>(&boot_artifacts, &mut map, "SHA-256")
                        .await?
                }
                ShowReferenceValueHashAlgo::Sha384 => {
                    inseart_reference_value::<sha2::Sha384>(&boot_artifacts, &mut map, "SHA-384")
                        .await?
                }
                ShowReferenceValueHashAlgo::Sm3 => {
                    inseart_reference_value::<sm3::Sm3>(&boot_artifacts, &mut map, "SM3").await?
                }
            }
        }

        if matches!(self.stage, Some(ShowReferenceValueStage::System)) {
            map.insert(
                format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_INITRD_SWITCH_ROOT}"),
                vec!["{}".to_string()],
            );
        }

        let json = serde_json::to_string_pretty(&map)?;

        println!("{json:#}");

        Ok(())
    }
}
