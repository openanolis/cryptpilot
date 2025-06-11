pub mod copy_config;
pub mod initrd_state;
pub mod metadata;

use std::path::Path;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use log::{error, info, warn};
use metadata::Metadata;
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    process::Command,
};

use crate::{
    cli::{BootServiceOptions, BootStage},
    cmd::show::PrintAsTable,
    config::{fde::RwOverlayType, volume::MakeFsType},
    fs::cmd::CheckCommandOutput,
    measure::{
        AutoDetectMeasure, Measure as _, OPERATION_NAME_FDE_ROOTFS_HASH,
        OPERATION_NAME_INITRD_SWITCH_ROOT,
    },
    provider::{IntoProvider as _, KeyProvider as _},
    types::IntegrityType,
};

use super::fde::disk::{FdeDisk as _, OnExternalFdeDisk};

const ROOTFS_LOGICAL_VOLUME: &'static str = "/dev/mapper/system-rootfs";
const ROOTFS_LAYER_NAME: &'static str = "rootfs";
const ROOTFS_LAYER_DEVICE: &'static str = "/dev/mapper/rootfs";
const ROOTFS_DECRYPTED_LAYER_DEVICE: &'static str = "/dev/mapper/rootfs_decrypted";
const ROOTFS_DECRYPTED_LAYER_NAME: &'static str = "rootfs_decrypted";
const ROOTFS_HASH_LOGICAL_VOLUME: &'static str = "/dev/mapper/system-rootfs_hash";
const DATA_LOGICAL_VOLUME: &'static str = "/dev/mapper/system-data";
const DATA_LAYER_NAME: &'static str = "data";
const DATA_LAYER_DEVICE: &'static str = "/dev/mapper/data";

pub struct BootServiceCommand {
    pub boot_service_options: BootServiceOptions,
}

#[async_trait]
impl super::Command for BootServiceCommand {
    async fn run(&self) -> Result<()> {
        match &self.boot_service_options.stage {
            BootStage::InitrdFdeBeforeSysroot => {
                setup_volumes_required_by_fde()
                    .await
                    .context("Failed to setup volumes required by FDE")?;
            }
            BootStage::SystemVolumesAutoOpen => {
                setup_user_provided_volumes(&self.boot_service_options)
                    .await
                    .context("Failed to setup volumes user provided automatically")?;
            }
            BootStage::InitrdFdeAfterSysroot => {
                let measure = AutoDetectMeasure::new().await;
                if let Err(e) = measure
                    .extend_measurement(OPERATION_NAME_INITRD_SWITCH_ROOT.into(), "{}".into()) // empty json object
                    .await
                    .context("Failed to record switch root event to runtime measurement")
                {
                    warn!("{e:?}")
                }

                setup_mounts_required_by_fde()
                    .await
                    .context("Failed to setup mounts required by FDE")?;
            }
        }

        info!("Everything have been completed, exit now");

        Ok(())
    }
}

async fn check_sysroot() -> Result<()> {
    // The mount of /sysroot is not done by cryptpilot. It is intentional, because we do not want to take over the job of /etc/fstab. So we have to check if sysroot is mounted from ROOTFS_LAYER_DEVICE.
    let mtab_content = fs::read_to_string("/etc/mtab").await?;
    for line in mtab_content.lines() {
        let mut fields = line.split(' ');
        match (fields.next(), fields.next()) {
            (Some(device), Some("/sysroot")) => {
                if device == ROOTFS_LAYER_DEVICE {
                    return Ok(());
                } else {
                    bail!("Rootfs mounted at /sysroot is not expected and could be a security risk. Expected: {ROOTFS_LAYER_DEVICE}, got: {device}");
                }
            }
            _ => continue,
        }
    }

    bail!("Failed to find the device mounted at /sysroot")
}

async fn load_metadata() -> Result<Metadata> {
    OnExternalFdeDisk::new_by_probing()
        .await?
        .load_metadata()
        .await
}

async fn setup_volumes_required_by_fde() -> Result<()> {
    let fde_config = crate::config::source::get_config_source()
        .await
        .get_fde_config()
        .await?;
    let Some(fde_config) = fde_config else {
        info!("The system is not configured for FDE, skip setting up now");
        return Ok(());
    };

    info!("Setting up volumes required by FDE");

    // 1. Checking and activating LVM volume group 'system'
    info!("[ 1/4 ] Checking and activating LVM volume group 'system'");
    Command::new("vgchange")
        .args(["-a", "y", "system"])
        .run()
        .await
        .context("Failed to activate LVM volume group 'system'")?;

    // 2. Load the root-hash and add it to the AAEL
    info!("[ 2/4 ] Loading root-hash");
    let metadata = load_metadata().await.context("Failed to load metadata")?;
    info!(
        "Got metadata type: {}, root-hash: {}",
        metadata.r#type, metadata.root_hash
    );
    if metadata.r#type != 1 {
        bail!("Unsupported cryptpilot metadata type: {}", metadata.r#type);
    }
    // Extend rootfs hash to runtime measurement
    let measure = AutoDetectMeasure::new().await;
    if let Err(e) = measure
        .extend_measurement(
            OPERATION_NAME_FDE_ROOTFS_HASH.into(),
            metadata.root_hash.clone(),
        )
        .await
        .context("Failed to extend rootfs hash to runtime measurement")
    {
        warn!("{e:?}")
    }

    // 3. Setup rootfs dm-crypt for rootfs volume
    info!("[ 3/4 ] Setting up rootfs volume");
    if let Some(encrypt) = &fde_config.rootfs.encrypt {
        // Setup dm-crypt for rootfs lv if required (optional)
        info!("Fetching passphrase for rootfs volume");
        let provider = encrypt.key_provider.clone().into_provider();
        let passphrase = provider
            .get_key()
            .await
            .context("Failed to get passphrase")?;

        info!("Setting up dm-crypt for rootfs volume");
        crate::fs::luks2::open(
            ROOTFS_DECRYPTED_LAYER_NAME,
            ROOTFS_LOGICAL_VOLUME,
            &passphrase,
            IntegrityType::None,
        )
        .await?;
    } else {
        info!("Encryption is disabled for rootfs volume, skip setting up dm-crypt")
    }

    info!("Setting up dm-verity for rootfs volume");
    setup_rootfs_dm_verity(
        &metadata.root_hash,
        if fde_config.rootfs.encrypt.is_some() {
            ROOTFS_DECRYPTED_LAYER_DEVICE
        } else {
            ROOTFS_LOGICAL_VOLUME
        },
    )
    .await?;

    // Now we have the rootfs ro part

    // 4. Open the data logical volume with dm-crypt and dm-integrity on it
    info!("[ 4/4 ] Setting up data volume");
    {
        // Check if the data logical volume exists
        let create_data_lv = !Path::new(DATA_LOGICAL_VOLUME).exists();
        if create_data_lv {
            info!(
                "Data logical volume does not exist, assume it is first time boot and create it."
            );

            // Due to there is no udev in initrd, the lvcreate will complain that /dev/system/data not exist. A workaround is to set '--zero n' and zeroing the first 4k of logical volume manually.
            // See https://serverfault.com/a/1059400
            async {
                Command::new("lvcreate")
                    .args([
                        "-n",
                        "data",
                        "--zero",
                        "n",
                        "-l",
                        "100%FREE",
                        "system",
                        "--nolocking",
                    ])
                    .run()
                    .await?;
                File::options()
                    .write(true)
                    .open("/dev/mapper/system-data")
                    .await?
                    .write_all(&[0u8; 4096])
                    .await?;
                Ok::<_, anyhow::Error>(())
            }
            .await
            .context("Failed to create data logical volume")?;
        }

        info!("Fetching passphrase for data volume");
        let provider = fde_config.data.encrypt.key_provider.into_provider();
        let passphrase = provider
            .get_key()
            .await
            .context("Failed to get passphrase")?;

        let integrity = if fde_config.data.integrity {
            IntegrityType::Journal // Select Journal mode since it is persistent storage
        } else {
            IntegrityType::None
        };

        if create_data_lv {
            // Create a LUKS volume on it
            info!("Creating LUKS2 on data volume");
            crate::fs::luks2::format(DATA_LOGICAL_VOLUME, &passphrase, integrity).await?;
        }

        crate::fs::luks2::open(DATA_LAYER_NAME, DATA_LOGICAL_VOLUME, &passphrase, integrity)
            .await?;

        if create_data_lv {
            // Create a Ext4 fs on it
            info!("Creating ext4 fs on data volume");
            crate::fs::luks2::makefs_if_empty(DATA_LAYER_NAME, &MakeFsType::Ext4, integrity)
                .await?;
        }
    }

    info!("Both rootfs volume and data volume are ready");

    Ok(())
}

async fn setup_mounts_required_by_fde() -> Result<()> {
    info!("Setting up mounts required by FDE");

    let fde_config = crate::config::source::get_config_source()
        .await
        .get_fde_config()
        .await?;
    let Some(fde_config) = fde_config else {
        info!("The system is not configured for FDE, skip setting up now");
        return Ok(());
    };

    check_sysroot().await?;

    // 1. Mount the data volume to filesystem
    info!("[ 1/4 ] Mounting data volume");
    async {
        tokio::fs::create_dir_all("/data_volume").await?;

        Command::new("mount")
            .arg(DATA_LAYER_DEVICE)
            .arg("/data_volume")
            .run()
            .await?;

        Ok::<_, anyhow::Error>(())
    }
    .await
    .context("Failed to mount data volume on /data_volume")?;

    // 2. Setup the rootfs-overlay. If on ram, create it first. If on disk, just use it to setup overlayfs.
    info!("[ 2/4 ] Setting up rootfs overlay");

    // Setup a backup of /sysroot at /sysroot_bak before mount overlay fs on it
    let sysroot_bak = Path::new("/sysroot_bak");
    async {
        tokio::fs::create_dir_all(sysroot_bak).await?;

        Command::new("mount")
            .arg("--bind")
            .arg("/sysroot")
            .arg(sysroot_bak)
            .arg("--make-private")
            .run()
            .await?;

        Ok::<_, anyhow::Error>(())
    }
    .await
    .with_context(|| format!("Failed to setup backup of /sysroot at {sysroot_bak:?}"))?;

    let overlay_type = fde_config.rootfs.rw_overlay.unwrap_or(RwOverlayType::Disk);

    // Load overlay module if not loaded
    tokio::task::spawn_blocking(|| {
        liblmod::modprobe(
            "overlay".to_string(),
            "".to_string(),
            liblmod::Selection::Current,
        )
        .or_else(|e| {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e);
            }
            Ok(())
        })
        .context("Failed to load kernel module 'overlay'")
    })
    .await??;

    let overlay_dir = match overlay_type {
        RwOverlayType::Ram => {
            info!("Using tmpfs as rootfs overlay");
            async {
                tokio::fs::create_dir_all("/ram_overlay").await?;

                Command::new("mount")
                    .args(["tmpfs", "-t", "tmpfs", "/ram_overlay"])
                    .run()
                    .await
                    .context("Failed to create tmpfs for rootfs overlay")?;

                tokio::fs::create_dir_all("/ram_overlay/upper").await?;
                tokio::fs::create_dir_all("/ram_overlay/work").await?;

                Command::new("mount")
                    .args(["-t", "overlay"])
                    .arg(ROOTFS_LAYER_DEVICE)
                    .args([
                        "-o",
                        "lowerdir=/sysroot,upperdir=/ram_overlay/upper,workdir=/ram_overlay/work",
                        "/sysroot",
                    ])
                    .run()
                    .await
                    .context("Failed to mount overlayfs")?;

                Ok::<_, anyhow::Error>(())
            }
            .await
            .context("Failed to setup overlayfs on /sysroot")?;

            Path::new("/ram_overlay")
        }
        RwOverlayType::Disk => {
            info!("Using data-volume:/overlay as rootfs overlay");
            async {
                tokio::fs::create_dir_all("/data_volume/overlay/upper").await?;
                tokio::fs::create_dir_all("/data_volume/overlay/work").await?;

                Command::new("mount")
                    .args(["-t", "overlay"])
                    .arg(ROOTFS_LAYER_DEVICE)
                    .args([
                        "-o",
                        "lowerdir=/sysroot,upperdir=/data_volume/overlay/upper,workdir=/data_volume/overlay/work",
                        "/sysroot",
                    ])
                    .run()
                    .await
                    .context("Failed to mount overlayfs")?;

                Ok::<_, anyhow::Error>(())
            }
            .await
            .context("Failed to setup overlayfs on /sysroot")?;

            Path::new("/data_volume/overlay")
        }
    };

    // Setting up mount bind for some special dirs
    info!("[ 3/4 ] Setting up mount bind");
    let dirs = [
        "/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/",
        "/var/lib/containers/storage/overlay/",
        "/var/lib/docker/overlay",
    ];

    for dir in dirs {
        // check if exist and not empty
        let task = async {
            let target = Path::new("/sysroot/").join(format!("./{dir}"));
            // Make sure the target dir is ready
            if target.exists() {
                if !target.is_dir() {
                    bail!("The target {target:?} exists but not a dir");
                }
            } else {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("Failed to create target dir {target:?}"))?;
            }
            // Create the original dir
            let origin = overlay_dir.join("mount-binds").join(format!("./{dir}"));
            if origin.exists() {
                if !origin.is_dir() {
                    bail!("The origin {origin:?} exists but not a dir");
                }
                // The origin dir is setting up previously
            } else {
                // First time to setup the origin dir, copy content to it.
                std::fs::create_dir_all(&origin)
                    .with_context(|| format!("Failed to create origin dir {origin:?}"))?;

                // We have to copy from the lower layer of the /sysroot
                let copy_source = sysroot_bak.join(format!("./{dir}"));
                if copy_source.exists() {
                    if let Err(e) = Command::new("cp")
                        .arg("-a")
                        .arg(format!("{copy_source:?}/."))
                        .arg(format!("{origin:?}/"))
                        .run()
                        .await
                        .with_context(|| {
                            format!("Failed to copy files from {copy_source:?} to {origin:?}")
                        })
                    {
                        let _ = std::fs::remove_dir_all(&origin);
                        Err(e)?
                    }
                }
            }

            // Mount bind
            Command::new("mount")
                .arg("--bind")
                .arg(&origin)
                .arg(&target)
                .run()
                .await
                .with_context(|| format!("Failed to setup mount bind on {target:?}"))?;

            Ok(())
        };

        if let Err(e) = task
            .await
            .with_context(|| format!("Failed to settiing up mount bind for {dir}"))
        {
            error!("{e:#}");
        }
    }

    // 4. mount --bind the /data folder
    info!("[ 4/4 ] Setting up user-data dir: /data");
    async {
        tokio::fs::create_dir_all("/data_volume/data").await?;
        tokio::fs::create_dir_all("/sysroot/data").await?;

        Command::new("mount")
            .args(["--bind", "/data_volume/data", "/sysroot/data"])
            .run()
            .await?;

        Ok::<_, anyhow::Error>(())
    }
    .await
    .context("Failed to setup mount bind on /sysroot/data")?;

    Ok(())
}

async fn setup_rootfs_dm_verity(root_hash: &str, lower_dm_device: &str) -> Result<()> {
    async {
        tokio::task::spawn_blocking(|| {
            liblmod::modprobe(
                "dm-verity".to_string(),
                "".to_string(),
                liblmod::Selection::Current,
            )
            .or_else(|e| {
                if e.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
                Ok(())
            })
            .context("Failed to load kernel module 'dm-verity'")
        })
        .await??;

        Command::new("veritysetup")
            .arg("open")
            .arg(lower_dm_device)
            .arg(ROOTFS_LAYER_NAME)
            .arg(ROOTFS_HASH_LOGICAL_VOLUME)
            .arg(root_hash)
            .run()
            .await?;

        Ok::<_, anyhow::Error>(())
    }
    .await
    .context("Failed to setup rootfs_verity")
}

async fn setup_user_provided_volumes(boot_service_options: &BootServiceOptions) -> Result<()> {
    info!("Checking status for all volumes now");
    let volume_configs = crate::config::source::get_config_source()
        .await
        .get_volume_configs()
        .await?;
    if volume_configs.len() == 0 {
        info!("The volume configs is empty, exit now");
        return Ok(());
    }
    volume_configs.print_as_table().await?;
    info!("Opening volumes according to volume configs");
    for volume_config in &volume_configs {
        match boot_service_options.stage {
            BootStage::InitrdFdeBeforeSysroot
                if volume_config.extra_config.auto_open != Some(true) =>
            {
                info!(
                    "Volume {} is skipped since 'auto_open = false'",
                    volume_config.volume
                );
                continue;
            }
            BootStage::InitrdFdeAfterSysroot => {
                unreachable!("This should never happen in initrd-fde-after-sysroot stage")
            }
            _ => { /* Accept */ }
        };

        info!(
            "Setting up mapping for volume {} from device {}",
            volume_config.volume, volume_config.dev
        );
        match super::open::open_for_specific_volume(&volume_config).await {
            Ok(_) => {
                info!(
                    "The mapping for volume {} is active now",
                    volume_config.volume
                );
            }
            Err(e) => {
                error!(
                    "Failed to setup mapping for volume {}: {e:?}",
                    volume_config.volume,
                )
            }
        };
    }
    info!("Checking status for all volumes again");
    volume_configs.print_as_table().await?;
    Ok(())
}
