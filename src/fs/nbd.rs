use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use block_devs::BlckExt as _;
use devicemapper::{DevId, DmName, DmOptions, DM};
use glob::{glob_with, MatchOptions};
use tokio::{
    fs::File,
    io::{AsyncReadExt as _, AsyncWriteExt},
    process::Command,
};

use crate::{async_defer, fs::cmd::CheckCommandOutput as _};

pub struct NbdDeviceNumber(u16);

impl NbdDeviceNumber {
    fn to_path(&self) -> PathBuf {
        format!("/dev/nbd{}", self.0).into()
    }
}

pub struct NbdDevice {
    nbd_dev_num: NbdDeviceNumber,
    #[allow(unused)]
    udev_rule: UdevRule,
}

impl NbdDevice {
    pub fn is_module_loaded() -> bool {
        Path::new("/dev/nbd0").exists()
    }

    pub async fn modprobe() -> Result<()> {
        Command::new("modprobe")
            .args(["nbd", "max_part=8"])
            .run()
            .await
            .context("Failed to load kernel module 'nbd'")?;

        Ok(())
    }

    pub async fn get_avaliable() -> Result<NbdDeviceNumber> {
        if !Self::is_module_loaded() {
            Self::modprobe().await?;
        }

        for i in 0..=99 {
            let dev = PathBuf::from(format!("/dev/nbd{i}"));
            if !dev.exists() {
                continue;
            }

            let Ok(file) = File::open(&dev).await else {
                continue;
            };

            let Ok(blk_size) = file.into_std().await.get_block_device_size() else {
                continue;
            };

            if blk_size == 0 {
                return Ok(NbdDeviceNumber(i));
            }
        }

        bail!("No available NBD device")
    }

    pub async fn connect(disk_img: impl AsRef<Path>) -> Result<Self> {
        let disk_img = disk_img.as_ref();
        if !disk_img.exists() {
            bail!("Disk image {disk_img:?} does not exist");
        }

        let nbd_dev_num = Self::get_avaliable().await?;
        let nbd_dev_path = nbd_dev_num.to_path();

        // The problem is that the nbd device may be use by the kernel (e.g. as mount point or as a device mapper) due to the annoying udev rules. Here we try to add a udev rule to ingore this device.
        let udev_rule = UdevRule::install_ignore_nbd_rule().await?;

        Command::new("qemu-nbd")
            .arg("--connect")
            .arg(&nbd_dev_path)
            .arg("--discard=on")
            .arg("--detect-zeroes=unmap")
            .arg(disk_img)
            .run()
            .await
            .with_context(|| {
                format!("Failed to connect disk image {disk_img:?} to NBD device {nbd_dev_path:?}")
            })?;

        tracing::debug!("Waiting 1 second for the nbd device to be ready");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        Ok(Self {
            nbd_dev_num,
            udev_rule,
        })
    }

    pub fn to_path(&self) -> PathBuf {
        self.nbd_dev_num.to_path()
    }

    async fn remove_holder_dm_devices(&self) -> Result<()> {
        let options = MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };

        let mut dm_names = vec![];
        for entry in glob_with(
            &format!(
                "/sys/block/nbd{}/nbd{}p*/holders/*/dm/name",
                self.nbd_dev_num.0, self.nbd_dev_num.0
            ),
            options,
        )
        .context("Failed to get dm devices with glob pattern")?
        {
            let mut dm_name = String::new();

            let entry = entry.context("Failed to get dm device path")?;
            File::open(&entry)
                .await
                .with_context(|| format!("Failed to open file {:?}", entry))?
                .read_to_string(&mut dm_name)
                .await
                .with_context(|| format!("Failed to read file {:?}", entry))?;

            dm_names.push(dm_name.trim_end().to_owned()); // Remove trailing newline
        }

        if !dm_names.is_empty() {
            tracing::debug!(
                ?dm_names,
                nbd_device = ?self.nbd_dev_num.to_path(),
                "Found dm devices related to the nbd device, remove them now"
            )
        }

        for device_name in dm_names {
            let dm = DM::new().context("Failed to communicating with device-mapper driver")?;
            let dm_name = DmName::new(&device_name)
                .with_context(|| format!("{device_name} is not a valid device name"))?;
            let dm_id = DevId::Name(dm_name);
            dm.device_remove(&dm_id, DmOptions::default())
                .with_context(|| format!("Failed to remove device-mapper device {device_name}"))?;
        }

        Ok(())
    }
}

impl Drop for NbdDevice {
    fn drop(&mut self) {
        let nbd_dev_path = self.to_path();
        async_defer! {
            async{


                if let Err(error) =  Command::new("qemu-nbd")
                    .arg("--disconnect")
                    .arg(&nbd_dev_path)
                    .run()
                    .await {
                    tracing::warn!(?error, "Failed to disconnect nbd device {nbd_dev_path:?}")
                };

                if let Err(error) =  self.remove_holder_dm_devices().await {
                    tracing::warn!(?error, "Failed to remove holders of nbd device {nbd_dev_path:?}")
                };

                Ok::<_, anyhow::Error>(())
            }
        }
    }
}

struct UdevRule {
    rule_path: PathBuf,
}

impl UdevRule {
    pub async fn install_ignore_nbd_rule() -> Result<Self> {
        let udev_rule_path = Path::new("/run/udev/rules.d");
        if !udev_rule_path.exists() {
            tracing::debug!("{udev_rule_path:?} does not exist, creating it");
            tokio::fs::create_dir_all(udev_rule_path)
                .await
                .with_context(|| format!("Failed to create {udev_rule_path:?}"))?;
        }

        which::which("udevadm").context("Could not found `udevadm`")?;

        let rule_path = udev_rule_path.join("99-cryptpilot-ignore.rules");

        let this = Self {
            rule_path: rule_path.to_path_buf(),
        };

        let mut file = File::options()
            .write(true)
            .create(true)
            .open(&rule_path)
            .await?;

        file.write_all(
            r#"
# Device used by CryptPilot
ACTION=="add|change", KERNEL=="nbd*", OPTIONS:="nowatch"
        "#
            .as_bytes(),
        )
        .await?;

        Command::new("udevadm")
            .arg("control")
            .arg("--reload-rules")
            .run()
            .await
            .context("Failed to reload udev rules")?;

        Command::new("udevadm")
            .arg("trigger")
            .run()
            .await
            .context("Failed to trigger udevadm")?;

        Ok(this)
    }
}

impl Drop for UdevRule {
    fn drop(&mut self) {
        let rule_path = self.rule_path.to_owned();
        async_defer! {
            async{
                if let Err(error) = tokio::fs::remove_file(&rule_path).await{
                    tracing::warn!(?error, "Failed to remove udev rule file")
                };

                if let Err(error) = Command::new("udevadm")
                    .arg("control")
                    .arg("--reload-rules")
                    .run()
                    .await {
                    tracing::warn!(?error, "Failed to reload udev rules")
                };

                if let Err(error) = Command::new("udevadm")
                    .arg("trigger")
                    .run()
                    .await {
                    tracing::warn!(?error, "Failed to trigger udevadm")
                };

            }
        }
    }
}
