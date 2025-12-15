use anyhow::{Context as _, Result};
use authenticode::PeTrait;
use futures::stream::StreamExt;
use indexmap::IndexMap;
use object::read::pe::{PeFile32, PeFile64};

use crate::{
    cmd::fde::disk::{grub::GrubArtifacts, kernel::KernelArtifacts},
    measure::{
        attestation_agent::AAEL_DOMAIN, OPERATION_NAME_FDE_ROOTFS_HASH, OPERATION_NAME_LOAD_CONFIG,
    },
};

#[derive(Debug)]
pub enum BootArtifacts {
    Grub {
        grub: GrubArtifacts,
        kernel: KernelArtifacts,
    },
}

pub async fn inseart_reference_value<T>(
    boot_artifacts: &[BootArtifacts],
    map: &mut IndexMap<String, Vec<String>>,
    hash_key: &str,
) -> Result<()>
where
    T: digest::Digest + digest::Update,
{
    map.insert(
        "kernel_cmdline".to_string(),
        boot_artifacts
            .iter()
            .flat_map(|BootArtifacts::Grub { grub: _, kernel }| {
                kernel
                    .kernel_cmdlines
                    .iter()
                    .map(|cmdline| format!("grub_kernel_cmdline {}", cmdline))
            })
            .collect::<Vec<_>>(),
    );

    map.insert(
        format!("measurement.kernel_cmdline.{hash_key}"),
        boot_artifacts
            .iter()
            .flat_map(|BootArtifacts::Grub { grub: _, kernel }| {
                kernel.kernel_cmdlines.iter().map(|cmdline| {
                    let mut hasher = T::new();
                    digest::Digest::update(&mut hasher, cmdline);
                    hex::encode(hasher.finalize())
                })
            })
            .collect::<Vec<_>>(),
    );

    map.insert(
        format!("measurement.kernel.{hash_key}"),
        boot_artifacts
            .iter()
            .map(|BootArtifacts::Grub { grub: _, kernel }| {
                let mut hasher = T::new();
                digest::Digest::update(&mut hasher, &kernel.kernel);
                hex::encode(hasher.finalize())
            })
            .collect::<Vec<_>>(),
    );

    map.insert(
        format!("measurement.initrd.{hash_key}"),
        boot_artifacts
            .iter()
            .map(|BootArtifacts::Grub { grub: _, kernel }| {
                let mut hasher = T::new();
                digest::Digest::update(&mut hasher, &kernel.initrd);
                hex::encode(hasher.finalize())
            })
            .collect::<Vec<_>>(),
    );

    map.insert(
        format!("measurement.grub.{hash_key}"),
        boot_artifacts
            .iter()
            .map(|BootArtifacts::Grub { grub, kernel: _ }| {
                calculate_authenticode_hash::<T>(&grub.grub_data)
            })
            .collect::<Result<Vec<_>>>()?,
    );

    map.insert(
        format!("measurement.shim.{hash_key}"),
        boot_artifacts
            .iter()
            .map(|BootArtifacts::Grub { grub, kernel: _ }| {
                calculate_authenticode_hash::<T>(&grub.shim_data)
            })
            .collect::<Result<Vec<_>>>()?,
    );

    {
        let (config_bundle_hash_hexs, root_hashes): (Vec<_>, Vec<_>) =
            futures::stream::iter(boot_artifacts.iter())
                .filter_map(|BootArtifacts::Grub { grub: _, kernel }| async {
                    kernel
                        .extract_cryptpilot_files()
                        .await
                        .and_then(|(fde_config_bundle, metadata)| {
                            Ok((fde_config_bundle.gen_hash_hex()?, metadata.root_hash))
                        })
                        .map_err(|error| {
                            tracing::warn!(
                            ?error,
                            "Failed to load fde config bundle or root_hash from initrd, skip now"
                        );
                        })
                        .ok()
                })
                .unzip()
                .await;

        {
            let aael_key = format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_LOAD_CONFIG}");
            map.insert(aael_key, config_bundle_hash_hexs);
        }

        {
            let aael_key = format!("AA.eventlog.{AAEL_DOMAIN}.{OPERATION_NAME_FDE_ROOTFS_HASH}");
            map.insert(aael_key, root_hashes);
        }
    }

    Ok(())
}

fn parse_pe(bytes: &[u8]) -> Result<Box<dyn PeTrait + '_>, object::read::Error> {
    if let Ok(pe) = PeFile64::parse(bytes) {
        Ok(Box::new(pe))
    } else {
        let pe = PeFile32::parse(bytes)?;
        Ok(Box::new(pe))
    }
}

fn calculate_authenticode_hash<T: digest::Digest + digest::Update>(bytes: &[u8]) -> Result<String> {
    let pe = parse_pe(bytes)?;
    let mut hasher = T::new();
    authenticode::authenticode_digest(&*pe, &mut hasher)
        .context("calculate_authenticode_hash failed")?;
    Ok(hex::encode(hasher.finalize()))
}
