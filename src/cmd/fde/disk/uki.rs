use std::path::Path;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use authenticode::PeTrait;
use object::{
    read::pe::{PeFile32, PeFile64},
    BinaryFormat, Object, ObjectSection,
};

use crate::cmd::fde::disk::{artifacts::BootArtifacts, kernel::KernelArtifacts, Disk};

pub const UKI_FILE_PATH_IN_EFI_PART: &str = "EFI/BOOT/BOOTX64.EFI";
pub const UKI_FILE_PATH: &str = "/boot/efi/EFI/BOOT/BOOTX64.EFI";

#[derive(Debug)]
pub struct UkiBootArtifacts {
    pub uki_data: Vec<u8>,
}

#[async_trait]
pub(super) trait FdeDiskUkiExt: Disk {
    async fn extract_boot_artifacts_uki(&self) -> Result<UkiBootArtifacts> {
        let uki_data = self.read_file_on_disk(Path::new(UKI_FILE_PATH)).await?;

        assume_uki_image(&uki_data)?;

        Ok(UkiBootArtifacts { uki_data })
    }
}

pub fn assume_uki_image(file_content: &[u8]) -> Result<()> {
    let uki_file = object::File::parse(file_content).context("Not a valid UKI file")?;

    if !matches!(uki_file.format(), BinaryFormat::Coff | BinaryFormat::Pe) {
        bail!(
            "Should be PE or COFF executable but got {:?}",
            uki_file.format()
        )
    }

    uki_file
        .section_by_name(".linux")
        .context("No .linux section found")?;

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

#[async_trait]

impl BootArtifacts for UkiBootArtifacts {
    async fn inseart_reference_value<T>(
        &self,
        map: &mut indexmap::IndexMap<String, Vec<String>>,
        hash_key: &str,
    ) -> Result<()>
    where
        T: digest::Digest + digest::Update,
    {
        map.insert(
            format!("measurement.uki.{hash_key}"),
            vec![calculate_authenticode_hash::<T>(&self.uki_data)?],
        );

        Ok(())
    }

    async fn extract_kernel_artifacts(&self) -> Result<Vec<KernelArtifacts>> {
        let uki_file = object::File::parse(&self.uki_data[..]).context("Not a valid UKI file")?;

        let cmdline = uki_file
            .section_by_name(".cmdline")
            .context("No .cmdline section found")?
            .data()?;
        let cmdline = std::str::from_utf8(cmdline).context("The cmdline is not valid UTF-8")?;

        let kernel = uki_file
            .section_by_name(".linux")
            .context("No .linux section found")?
            .data()?;

        let initrd = uki_file
            .section_by_name(".initrd")
            .context("No .initrd section found")?
            .data()?;

        Ok(vec![KernelArtifacts {
            kernel_cmdlines: vec![cmdline.to_owned()],
            kernel: kernel.to_owned(),
            initrd: initrd.to_owned(),
        }])
    }
}
