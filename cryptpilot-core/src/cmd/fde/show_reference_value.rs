use std::path::PathBuf;

use anyhow::{bail, Result};
use async_trait::async_trait;
use indexmap::IndexMap;

use crate::{
    cli::ShowReferenceValueHashAlgo,
    cmd::fde::disk::{
        artifacts::BootArtifacts, current::OnCurrentSystemFdeDisk, external::OnExternalFdeDisk,
        BootArtifactsType,
    },
};

use super::disk::FdeDisk;

pub struct ShowReferenceValueCommand {
    pub disk: Option<PathBuf>,
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

        let boot_artifacts = fde_disk.extract_boot_artifacts().await?;
        tracing::debug!("Starting to calculate reference values");

        match boot_artifacts {
            BootArtifactsType::Grub(grub_boot_artifacts) => {
                common_insert(&grub_boot_artifacts, &mut map, &self.hash_algos).await?;
            }
            BootArtifactsType::Uki(uki_boot_artifacts) => {
                common_insert(&uki_boot_artifacts, &mut map, &self.hash_algos).await?;
            }
        };

        let json = serde_json::to_string_pretty(&map)?;

        println!("{json:#}");

        Ok(())
    }
}

async fn common_insert(
    boot_artifacts: &impl BootArtifacts,
    map: &mut IndexMap<String, Vec<String>>,
    hash_algos: &[ShowReferenceValueHashAlgo],
) -> Result<()> {
    for hash_algo in hash_algos {
        match hash_algo {
            ShowReferenceValueHashAlgo::Sha1 => {
                boot_artifacts
                    .inseart_reference_value::<sha1::Sha1>(map, "SHA-1")
                    .await?
            }
            ShowReferenceValueHashAlgo::Sha256 => {
                boot_artifacts
                    .inseart_reference_value::<sha2::Sha256>(map, "SHA-256")
                    .await?
            }
            ShowReferenceValueHashAlgo::Sha384 => {
                boot_artifacts
                    .inseart_reference_value::<sha2::Sha384>(map, "SHA-384")
                    .await?
            }
            ShowReferenceValueHashAlgo::Sm3 => {
                boot_artifacts
                    .inseart_reference_value::<sm3::Sm3>(map, "SM3")
                    .await?
            }
        }
    }
    Ok(())
}
