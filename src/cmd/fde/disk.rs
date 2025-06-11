use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use block_devs::BlckExt;
use tokio::{fs::File, process::Command};

use crate::{
    cmd::boot_service::metadata::Metadata,
    config::source::{cloud_init::FdeConfigBundle, fs::FileSystemConfigSource, ConfigSource},
    fs::{cmd::CheckCommandOutput as _, mount::TmpMountPoint, nbd::NbdDevice},
};

#[async_trait]
pub trait FdeDisk {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle>;

    async fn load_metadata(&self) -> Result<Metadata>;
}

const CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT: &'static str = "cryptpilot/config";
const METADATA_PATH_IN_BOOT: &'static str = "cryptpilot/metadata.toml";

async fn load_fde_config_bundle_from_dir(config_dir: &Path) -> Result<FdeConfigBundle> {
    Ok(FileSystemConfigSource::new(config_dir)
        .get_config()
        .await
        .with_context(|| format!("Can not read config dir at {config_dir:?}"))?
        .strip_as_fde_config_bundle())
}

async fn load_metadata_from_file(metadata_path: &Path) -> Result<Metadata> {
    let metadata_content = tokio::fs::read_to_string(&metadata_path)
        .await
        .with_context(|| format!("Can not read metadata file at {metadata_path:?}"))?;
    let mut metadata = toml::from_str::<Metadata>(&metadata_content)?;

    tracing::debug!("Metadata content:\n{}", metadata_content);

    // Sanity check on root_hash, since it is from unsafe source
    let root_hash_bin = hex::decode(metadata.root_hash).context("Bad root hash")?;
    metadata.root_hash = hex::encode(root_hash_bin);

    Ok(metadata)
}

/// Load the fde related config bundle from current system. This should be used
/// only when the system is booted into the system manager (systemd) stage, and
/// should not be used in initrd stage.
#[non_exhaustive]
pub struct OnCurrentSystemFdeDisk {}

impl OnCurrentSystemFdeDisk {
    pub async fn new() -> Result<Self> {
        if !Path::new("/boot").join(METADATA_PATH_IN_BOOT).exists() {
            bail!("Metadata file not found in /boot.\nThe current system may not be booted from an encrypted disk. You can follow the instructions here to it encrypt it first: https://github.com/openanolis/cryptpilot")
        }
        Ok(Self {})
    }
}

#[async_trait]
impl FdeDisk for OnCurrentSystemFdeDisk {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        load_fde_config_bundle_from_dir(
            &Path::new("/boot").join(CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT),
        )
        .await
    }

    async fn load_metadata(&self) -> Result<Metadata> {
        load_metadata_from_file(&Path::new("/boot").join(METADATA_PATH_IN_BOOT)).await
    }
}

/// Load the fde related config bundle from a disk device.
pub struct OnExternalFdeDisk {
    #[allow(unused)]
    nbd_device: Option<NbdDevice>,
    boot_dev_tmp_mount: TmpMountPoint,
}

impl OnExternalFdeDisk {
    pub async fn new_from_disk(disk: &Path) -> Result<Self> {
        if !disk.exists() {
            bail!("File not exist: {disk:?}")
        }

        let real_block_device = File::open(&disk).await?.into_std().await.is_block_device();

        let (nbd_device, disk_device) = if real_block_device {
            (None, disk.to_owned())
        } else {
            // Theat it as a disk image file
            tracing::debug!(
                "The path {disk:?} is not a block device, treat it as a disk image file."
            );
            let nbd_device = NbdDevice::connect(disk).await?;
            let disk_device = nbd_device.to_path();
            (Some(nbd_device), disk_device)
        };

        // Find the boot partition and mount it to a tmp mount point
        let boot_dev = Self::detect_boot_part(Some(&disk_device)).await.context(
            "Cannot found boot partition on the disk. The disk may not be a encrypted disk.",
        )?;
        let boot_dev_tmp_mount = TmpMountPoint::mount(boot_dev).await?;

        Ok(Self {
            nbd_device,
            boot_dev_tmp_mount,
        })
    }

    /// New by probing the boot partition on current environment. This is used in initrd stage.
    pub async fn new_by_probing() -> Result<Self> {
        let boot_dev = Self::detect_boot_part(None).await?;
        let boot_dev_tmp_mount = TmpMountPoint::mount(boot_dev).await?;

        Ok(Self {
            nbd_device: None,
            boot_dev_tmp_mount,
        })
    }

    async fn detect_boot_part(hint_device: Option<&Path>) -> Result<PathBuf> {
        let mut cmd = Command::new("blkid");
        cmd.args(["--match-types", "ext4"])
            .args(["--match-token", r#"PARTLABEL="boot""#])
            .args(["--list-one", "--output", "device"]);

        if let Some(hint_device) = hint_device {
            cmd.arg(hint_device);
        };

        cmd.run()
            .await
            .and_then(|stdout| {
                let mut device_name = String::from_utf8(stdout)?;
                device_name = device_name.trim().into();
                if device_name.is_empty() {
                    bail!("No boot partition found");
                }
                Ok(PathBuf::from(device_name))
            })
            .context("Failed to detect boot partition")
    }
}

#[async_trait]
impl FdeDisk for OnExternalFdeDisk {
    async fn load_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        let config_dir = self
            .boot_dev_tmp_mount
            .mount_point()
            .join("boot")
            .join(CRYPTPILOT_CONFIG_DIR_UNTRUSTED_IN_BOOT);
        if !config_dir.exists() {
            bail!("No config dir found in boot partition. The disk may not be a encrypted disk.")
        }
        load_fde_config_bundle_from_dir(&config_dir).await
    }

    async fn load_metadata(&self) -> Result<Metadata> {
        let metadata_file = self
            .boot_dev_tmp_mount
            .mount_point()
            .join("boot")
            .join(METADATA_PATH_IN_BOOT);

        if !metadata_file.exists() {
            bail!("No metadata file found in boot partition. The disk may not be a encrypted disk.")
        }

        load_metadata_from_file(&metadata_file).await
    }
}
