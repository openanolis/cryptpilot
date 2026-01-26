use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::{
    fs::{self},
    process::Command,
};

use crate::cmd::boot_service::stage::{DATA_LAYER_DEVICE, ROOTFS_LAYER_DEVICE};
use cryptpilot::fs::cmd::CheckCommandOutput;

use crate::config::RwOverlayType;

pub async fn setup_mounts_required_by_fde() -> Result<()> {
    tracing::info!("Setting up mounts required by FDE");

    let fde_config = crate::config::get_fde_config_source()
        .await
        .get_fde_config()
        .await?;
    let Some(fde_config) = fde_config else {
        tracing::info!("The system is not configured for FDE, skip setting up now");
        return Ok(());
    };

    check_sysroot().await?;

    // 1. Mount the data volume to filesystem
    tracing::info!("[ 1/4 ] Mounting data volume");
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
    tracing::info!("[ 2/4 ] Setting up rootfs overlay");

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
    Command::new("modprobe")
        .arg("overlay")
        .run()
        .await
        .context("Failed to load kernel module 'overlay'")?;

    let overlay_dir = match overlay_type {
        RwOverlayType::Ram => {
            tracing::info!("Using tmpfs as rootfs overlay");
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
        RwOverlayType::Disk | RwOverlayType::DiskPersist => {
            let should_clear = matches!(overlay_type, RwOverlayType::Disk);
            if should_clear {
                tracing::info!(
                    "Using data-volume:/overlay as rootfs overlay (ephemeral mode, will be cleared on boot)"
                );
            } else {
                tracing::info!("Using data-volume:/overlay as rootfs overlay (persistent mode)");
            }
            async {
                let overlay_path = Path::new("/data_volume/overlay");

                // If disk mode (default), clear the overlay directory on boot
                if should_clear && overlay_path.exists() {
                    tracing::info!("Clearing overlay directory for ephemeral mode");
                    if let Err(e) = tokio::fs::remove_dir_all(overlay_path).await {
                        tracing::warn!(
                            "Failed to clear overlay directory: {:#}. Continuing anyway.",
                            e
                        );
                    }
                }

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
    tracing::info!("[ 3/4 ] Setting up mount bind");
    let dirs = [
        "/var/lib/containerd/io.containerd.snapshotter.v1.overlayfs/snapshots/",
        "/var/lib/containers/",
        "/var/lib/docker/",
    ];

    for dir in dirs {
        tracing::info!("Setting up mount bind for {dir}");
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
                        .arg(copy_source.join("."))
                        .arg(&origin)
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
            tracing::error!("{e:#}");
        }
    }

    // 4. mount --bind the /data folder
    tracing::info!("[ 4/4 ] Setting up user-data dir: /data");
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
