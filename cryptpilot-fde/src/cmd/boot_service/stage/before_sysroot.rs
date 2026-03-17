use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tokio::{fs::File, io::AsyncWriteExt, process::Command};

use crate::{
    cmd::boot_service::{
        metadata::{load_metadata_from_file, Metadata},
        stage::{
            DATA_DEVICE, DATA_LOGICAL_VOLUME, DATA_NAME, ROOTFS_DECRYPTED_LAYER_DEVICE,
            ROOTFS_DECRYPTED_NAME, ROOTFS_DEVICE, ROOTFS_EXTENDED_DEVICE, ROOTFS_EXTENDED_NAME,
            ROOTFS_HASH_LOGICAL_VOLUME, ROOTFS_LOGICAL_VOLUME, ROOTFS_NAME, ROOTFS_VERITY_DEVICE,
            ROOTFS_VERITY_NAME,
        },
    },
    config::{RwOverlayBackend, RwOverlayLocation},
};
use block_devs::BlckExt;
use cryptpilot::{
    fs::cmd::CheckCommandOutput,
    provider::{IntoProvider as _, KeyProvider as _, VolumeType},
    types::{IntegrityType, MakeFsType},
};

const CRYPTPILOT_LVM_SYSTEM_DIR: &str = "/usr/lib/cryptpilot/lvm/";
pub const METADATA_PATH_IN_INITRD: &str = "/etc/cryptpilot/metadata.toml";

pub async fn setup_volumes_required_by_fde() -> Result<()> {
    let fde_config = crate::config::get_fde_config_source()
        .await
        .get_fde_config()
        .await?;
    let Some(fde_config) = fde_config else {
        tracing::info!("The system is not configured for FDE, skip setting up now");
        return Ok(());
    };

    tracing::info!("Setting up volumes required by FDE");

    // 1. Checking and activating LVM volume group 'system'
    tracing::info!("[ 1/4 ] Checking and activating LVM volume group 'system'");
    Command::new("vgchange")
        .args(["-a", "y", "system"])
        .run()
        .await
        .context("Failed to activate LVM volume group 'system'")?;

    // 2. Load the root-hash and add it to the AAEL
    tracing::info!("[ 2/4 ] Loading root-hash");
    let metadata = load_metadata().await.context("Failed to load metadata")?;
    tracing::info!(
        "Got metadata type: {}, root-hash: {}",
        metadata.r#type,
        metadata.root_hash
    );
    if metadata.r#type != 1 {
        bail!("Unsupported cryptpilot metadata type: {}", metadata.r#type);
    }

    // 3. Setup rootfs dm-crypt for rootfs volume
    tracing::info!("[ 3/4 ] Setting up rootfs volume");
    if let Some(encrypt) = &fde_config.rootfs.encrypt {
        // Setup dm-crypt for rootfs lv if required (optional)
        tracing::info!("Fetching passphrase for rootfs volume");
        let provider = encrypt.key_provider.clone().into_provider();

        if matches!(provider.volume_type(), VolumeType::Temporary) {
            bail!(
                "Key provider {:?} is not supported for rootfs volume",
                provider.debug_name()
            )
        }

        let passphrase = provider
            .get_key()
            .await
            .context("Failed to get passphrase")?;

        tracing::info!("Setting up dm-crypt for rootfs volume");
        cryptpilot::fs::luks2::open_with_check_passphrase(
            ROOTFS_DECRYPTED_NAME,
            Path::new(ROOTFS_LOGICAL_VOLUME),
            &passphrase,
            IntegrityType::None,
        )
        .await?;
    } else {
        tracing::info!("Encryption is disabled for rootfs volume, skip setting up dm-crypt")
    }

    tracing::info!("Setting up dm-verity for rootfs volume");

    let backend = fde_config.rootfs.rw_overlay_backend.unwrap_or_default();

    let (dm_verity_output_name, dm_verity_output_device) = match backend {
        RwOverlayBackend::Overlayfs => (ROOTFS_NAME, Path::new(ROOTFS_DEVICE)),
        RwOverlayBackend::DmSnapshot => (ROOTFS_VERITY_NAME, Path::new(ROOTFS_VERITY_DEVICE)),
    };

    setup_rootfs_dm_verity(
        dm_verity_output_name,
        &metadata.root_hash,
        Path::new(if fde_config.rootfs.encrypt.is_some() {
            ROOTFS_DECRYPTED_LAYER_DEVICE
        } else {
            ROOTFS_LOGICAL_VOLUME
        }),
    )
    .await?;
    // Now we have the rootfs ro part

    // 4. Setup data volume and overlay backend
    {
        let rw_overlay_location = fde_config
            .rootfs
            .rw_overlay_location
            .unwrap_or(RwOverlayLocation::Disk);

        tracing::info!(
            ?backend,
            ?rw_overlay_location,
            "[ 4/4 ] Setting up data volume if required"
        );

        if matches!(
            rw_overlay_location,
            RwOverlayLocation::Disk | RwOverlayLocation::DiskPersist
        ) {
            tracing::info!("Expanding system PV partition");
            if let Err(error) = expand_system_pv_partition().await {
                tracing::warn!(?error, "Failed to expend the system PV partition");
            }

            // Ensure data logical volume exists
            ensure_data_volume_exist_and_expanded().await?;

            let (recreate, integrity) =
                setup_data_volume_luks2(&fde_config.data, rw_overlay_location).await?;

            // Setup data volume based on backend type
            match backend {
                RwOverlayBackend::Overlayfs => {
                    let data_device = Path::new(DATA_DEVICE);
                    if recreate {
                        tracing::info!("Creating ext4 fs on data volume");
                        cryptpilot::fs::mkfs::force_mkfs(data_device, &MakeFsType::Ext4, integrity)
                            .await?;
                    } else {
                        // Resize existing filesystem to fill the expanded device
                        resize_ext4_filesystem(data_device).await?;
                    }
                }
                RwOverlayBackend::DmSnapshot => {
                    // Build dm-snapshot device chain
                    setup_dm_snapshot_device_chain(
                        dm_verity_output_device,
                        Path::new(DATA_DEVICE),
                        matches!(rw_overlay_location, RwOverlayLocation::DiskPersist),
                    )
                    .await?;

                    // Resize rootfs filesystem to fill the expanded device after building snapshot chain
                    resize_ext4_filesystem(Path::new(ROOTFS_DEVICE)).await?;
                }
            }
        } else {
            // No need to set up data volume
            match backend {
                RwOverlayBackend::Overlayfs => {
                    // Nothing to do
                }
                RwOverlayBackend::DmSnapshot => {
                    tracing::info!("Creating zram device for COW storage");
                    let cow_device = create_zram_cow_device().await?;
                    // Build dm-snapshot device chain
                    setup_dm_snapshot_device_chain(dm_verity_output_device, &cow_device, false)
                        .await?;
                    // Resize rootfs filesystem to fill the expanded device after building snapshot chain
                    resize_ext4_filesystem(Path::new(ROOTFS_DEVICE)).await?;
                }
            }
        }
    }

    tracing::info!("Both rootfs volume and data volume are ready");

    Ok(())
}

async fn ensure_data_volume_exist_and_expanded() -> Result<(), anyhow::Error> {
    Ok(if !Path::new(DATA_LOGICAL_VOLUME).exists() {
        tracing::info!(
            "Data logical volume does not exist, assume it is first time boot and create it."
        );

        // Due to there is no udev in initrd, the lvcreate will complain that /dev/system/data not exist. A workaround is to set '--zero n' and zeroing the first 4k of logical volume manually.
        // See https://serverfault.com/a/1059400
        async {
            Command::new("lvcreate")
                .args(["-n", DATA_NAME, "--zero", "n", "-l", "100%FREE", "system"])
                .env("LVM_SYSTEM_DIR", CRYPTPILOT_LVM_SYSTEM_DIR)
                .run()
                .await?;
            File::options()
                .write(true)
                .open(DATA_LOGICAL_VOLUME)
                .await?
                .write_all(&[0u8; 4096])
                .await?;
            Ok::<_, anyhow::Error>(())
        }
        .await
        .context("Failed to create data logical volume")?;
    } else {
        tracing::info!("Expanding data logical volume");
        if let Err(error) = expand_system_data_lv().await {
            tracing::warn!(?error, "Failed to expend data logical volume");
        }
    })
}

async fn load_metadata() -> Result<Metadata> {
    load_metadata_from_file(Path::new(METADATA_PATH_IN_INITRD)).await
}

async fn setup_rootfs_dm_verity(
    dm_verity_output_name: &str,
    root_hash: &str,
    lower_dm_device: &Path,
) -> Result<()> {
    async {
        Command::new("modprobe")
            .arg("dm-verity")
            .run()
            .await
            .context("Failed to load kernel module 'dm-verity'")?;

        Command::new("veritysetup")
            .arg("open")
            .arg(lower_dm_device)
            .arg(dm_verity_output_name)
            .arg(ROOTFS_HASH_LOGICAL_VOLUME)
            .arg(root_hash)
            .run()
            .await?;

        Ok::<_, anyhow::Error>(())
    }
    .await
    .context("Failed to setup rootfs_verity")
}

async fn expand_system_pv_partition() -> Result<()> {
    Command::new("bash")
        .arg("-c")
        .arg(
            r#"
set -euo pipefail

VG_NAME="system"

# Find any PV belonging to the volume group
PV_DEV=$(pvs --noheadings -o pv_name,vg_name | awk "\$2==\"$VG_NAME\" {print \$1; exit}")

if [[ -z "$PV_DEV" ]]; then
    echo "Error: No physical volume found for volume group '$VG_NAME'" >&2
    exit 1
fi

# Get the parent disk (e.g. nvme0n1)
DISK_DEV=$(lsblk -dno PKNAME "$PV_DEV")
DISK_PATH="/dev/$DISK_DEV"

if [[ ! -b "$DISK_PATH" ]]; then
    echo "Error: Disk device not found: $DISK_PATH" >&2
    exit 1
fi

echo "Volume group '$VG_NAME' uses PV: $PV_DEV"
echo "Located on disk: $DISK_PATH"

# Get the last partition number
LAST_PART_NUM=$(lsblk -nro NAME "$DISK_PATH" |
    grep -E "^${DISK_DEV}[p]*[0-9]+$" |
    tail -1 |
    sed -E "s/^${DISK_DEV}[p]*//")

if [[ -z "$LAST_PART_NUM" ]]; then
    echo "Error: Failed to detect last partition on $DISK_PATH" >&2
    exit 1
fi

echo "Last partition number: $LAST_PART_NUM"

echo "Expanding partition and physical volume ..."
if growpart "$DISK_PATH" "$LAST_PART_NUM"; then
    # the growpart command fill also call lvm pvresize to resize the related data volume
    echo "Physical volume resized successfully"

elif [[ $? -eq 1 ]]; then
    # return 1 means no more space available
    echo "No action: partition $LAST_PART_NUM is already at maximum size."
else
    echo "ERROR: growpart failed unexpectedly." >&2
    exit 1
fi
            "#,
        )
        .env("LVM_SYSTEM_DIR", CRYPTPILOT_LVM_SYSTEM_DIR)
        .run()
        .await?;

    Ok::<_, anyhow::Error>(())
}

async fn expand_system_data_lv() -> Result<()> {
    Command::new("lvextend")
        .arg("-l")
        .arg("+100%FREE")
        .arg(DATA_LOGICAL_VOLUME)
        .env("LVM_SYSTEM_DIR", CRYPTPILOT_LVM_SYSTEM_DIR)
        .run_with_status_checker(|code, _, _| match code {
            0 | 5 => Ok(()),
            _ => {
                bail!("Bad exit code")
            }
        })
        .await?;

    Ok::<_, anyhow::Error>(())
}

/// Setup data volume LUKS2 encryption and return whether content should be recreated
async fn setup_data_volume_luks2(
    data_config: &crate::config::DataConfig,
    rw_overlay_location: RwOverlayLocation,
) -> Result<(bool, IntegrityType)> {
    tracing::info!("Fetching passphrase for data volume");
    let provider = data_config.encrypt.key_provider.clone().into_provider();
    let passphrase = provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;

    let integrity = if data_config.integrity {
        IntegrityType::Journal // Select Journal mode since it is persistent storage
    } else {
        IntegrityType::None
    };

    let data_logical_volume_dev = Path::new(DATA_LOGICAL_VOLUME);

    let recreate = if matches!(provider.volume_type(), VolumeType::Temporary) {
        tracing::info!("Key provider is temporary, will recreate data volume content");
        true
    } else if !cryptpilot::fs::luks2::is_initialized(data_logical_volume_dev).await? {
        tracing::info!("Data volume is not initialized, will create new content");
        true
    } else if matches!(rw_overlay_location, RwOverlayLocation::Disk) {
        tracing::info!("Overlay type is disk (non-persistent), will recreate data volume content");
        true
    } else {
        tracing::info!("Data volume is initialized and overlay type is persistent, will reuse existing content");
        false
    };

    if recreate {
        // Create a LUKS volume on it
        tracing::info!("Creating LUKS2 on data volume");
        cryptpilot::fs::luks2::format(data_logical_volume_dev, &passphrase, integrity).await?;
    }

    // TODO: support change size of the LUKS2 volume and inner ext4 file system
    tracing::info!("Opening data volume");
    cryptpilot::fs::luks2::open_with_check_passphrase(
        DATA_NAME,
        data_logical_volume_dev,
        &passphrase,
        integrity,
    )
    .await?;

    Ok((recreate, integrity))
}

async fn create_zram_cow_device() -> Result<PathBuf> {
    // Load zram module
    if !Path::new("/sys/class/zram-control").exists() {
        Command::new("modprobe")
            .arg("zram")
            .run()
            .await
            .context("Failed to load zram module")?;
    }

    // Get total memory in KB
    let mem_info = tokio::fs::read_to_string("/proc/meminfo").await?;
    let mem_total_kb = mem_info
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .context("Failed to parse MemTotal from /proc/meminfo")?;

    // Add a zram device
    let zram_id = tokio::fs::read_to_string("/sys/class/zram-control/hot_add")
        .await
        .context("Adding zram device")?
        .trim_end()
        .parse::<u64>()
        .context("Allocate new zram device number")?;

    // Set zram size equal to total memory
    let zram_size = format!("{}K", mem_total_kb);
    tokio::fs::write(format!("/sys/block/zram{}/disksize", zram_id), &zram_size)
        .await
        .context("Failed to set zram disksize")?;
    tracing::info!(size = %zram_size, "Created zram{}", zram_id);

    Ok(PathBuf::from(format!("/dev/zram{}", zram_id)))
}

async fn setup_dm_snapshot_device_chain(
    rootfs_device: &Path,
    cow_device: &Path,
    persistent: bool,
) -> Result<()> {
    tracing::info!(
        ?rootfs_device,
        ?cow_device,
        "Building dm-snapshot device chain"
    );

    // Load required kernel modules
    Command::new("modprobe")
        .arg("dm-snapshot")
        .run()
        .await
        .context("Failed to load dm-snapshot module")?;

    // Get device sizes (in sectors, 512 bytes each)
    let verity_size = get_device_size_bytes(rootfs_device).await? / 512;
    let cow_size = get_device_size_bytes(cow_device).await? / 512;

    // Create dm-linear device combining dm-verity and zero target
    // The zero target is used directly in the table, no need to create a separate dm-zero device
    let linear_size = verity_size + cow_size;
    tracing::info!(
        "Creating dm-linear device with {} sectors (verity:{} + zero:{})",
        linear_size,
        verity_size,
        cow_size
    );
    Command::new("dmsetup")
        .arg("create")
        .arg(ROOTFS_EXTENDED_NAME)
        .arg("--table")
        .arg(format!(
            "0 {} linear {} 0\n{} {} zero",
            verity_size,
            rootfs_device.to_string_lossy(),
            verity_size,
            cow_size
        ))
        .run()
        .await
        .context("Failed to create dm-linear device")?;

    // Create dm-snapshot device
    tracing::info!("Creating dm-snapshot device");
    Command::new("dmsetup")
        .arg("create")
        .arg(ROOTFS_NAME)
        .arg("--table")
        .arg(format!(
            "0 {} snapshot {} {} {} 16", // chunk size is 16 sectors (8KB)
            linear_size,
            ROOTFS_EXTENDED_DEVICE,
            cow_device.to_string_lossy(),
            if persistent { "PO" } else { "N" }
        ))
        .run()
        .await
        .context("Failed to create dm-snapshot device")?;

    tracing::info!("dm-snapshot device chain created successfully");
    Ok(())
}

async fn get_device_size_bytes(device: &Path) -> Result<u64> {
    let file = File::open(device)
        .await
        .context(format!("Failed to open device {:?}", device))?
        .into_std()
        .await;

    file.get_block_device_size().context(format!(
        "Failed to get block device size in bytes {:?}",
        device
    ))
}

async fn resize_ext4_filesystem(device: &Path) -> Result<()> {
    tracing::info!(device = %device.display(), "Resizing ext4 filesystem to fill device");

    // Clear the read-only feature flag before resizing
    Command::new("tune2fs")
        .args(["-O", "^read-only"])
        .arg(device)
        .run()
        .await
        .context(format!(
            "Failed to clear read-only flag on {}",
            device.display()
        ))?;

    Command::new("resize2fs")
        .arg(device)
        .run()
        .await
        .context(format!(
            "Failed to resize ext4 filesystem on {}",
            device.display()
        ))?;
    tracing::info!("ext4 filesystem resized successfully");
    Ok(())
}
