use std::path::PathBuf;

use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::StreamExt;
use indexmap::IndexMap;

use crate::{
    cli::{ShowReferenceValueHashAlgo, ShowReferenceValueStage},
    cmd::fde::disk::{
        artifacts::BootArtifacts, current::OnCurrentSystemFdeDisk, external::OnExternalFdeDisk,
        BootArtifactsType,
    },
    measure::{
        attestation_agent::AAEL_DOMAIN, OPERATION_NAME_FDE_ROOTFS_HASH,
        OPERATION_NAME_INITRD_SWITCH_ROOT, OPERATION_NAME_LOAD_CONFIG,
    },
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

        let boot_artifacts = fde_disk.extract_boot_artifacts().await?;
        tracing::debug!("Starting to calculate reference values");

        let kernel_artifacts = match boot_artifacts {
            BootArtifactsType::Grub(grub_boot_artifacts) => {
                common_insert(&grub_boot_artifacts, &mut map, &self.hash_algos).await?;
                grub_boot_artifacts.extract_kernel_artifacts().await?
            }
            BootArtifactsType::Uki(uki_boot_artifacts) => {
                common_insert(&uki_boot_artifacts, &mut map, &self.hash_algos).await?;
                uki_boot_artifacts.extract_kernel_artifacts().await?
            }
        };

        {
            let (config_bundle_hash_hexs, root_hashes): (Vec<_>, Vec<_>) =
                futures::stream::iter(kernel_artifacts.into_iter())
                    .filter_map(|kernel| async move {
                        kernel
                            .extract_cryptpilot_files()
                            .await
                            .and_then(|(fde_config_bundle, metadata)| {
                                Ok((fde_config_bundle.gen_hash_hex()?, metadata.root_hash))
                            })
                            .map_err(|error| {
                                #[rustfmt::skip]
                                tracing::warn!(
                                    ?error,
                                    "Failed to load fde config bundle or root_hash from initrd, skip now"
                                );
                            })
                            .ok()
                    })
                    .unzip()
                    .await;

            if !config_bundle_hash_hexs.is_empty() {
                {
                    let aael_key =
                        format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_LOAD_CONFIG}");
                    map.insert(aael_key, config_bundle_hash_hexs);
                }

                {
                    let aael_key =
                        format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_FDE_ROOTFS_HASH}");
                    map.insert(aael_key, root_hashes);
                }

                if matches!(self.stage, Some(ShowReferenceValueStage::System)) {
                    map.insert(
                        format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_INITRD_SWITCH_ROOT}"),
                        vec!["{}".to_string()],
                    );
                }
            }
        }

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
