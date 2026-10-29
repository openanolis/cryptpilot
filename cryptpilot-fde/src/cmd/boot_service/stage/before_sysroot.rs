use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tokio::{fs::File, io::AsyncWriteExt, process::Command};

use crate::{
    cmd::boot_service::{
        metadata::{load_metadata_from_file, Metadata, METADATA_PATH_IN_INITRD},
        stage::{
            DELTA_DEVICE, DELTA_LOGICAL_VOLUME, DELTA_NAME, ROOTFS_DECRYPTED_LAYER_DEVICE,
            ROOTFS_DECRYPTED_NAME, ROOTFS_DEVICE, ROOTFS_EXTENDED_DEVICE, ROOTFS_EXTENDED_NAME,
            ROOTFS_HASH_LOGICAL_VOLUME, ROOTFS_LOGICAL_VOLUME, ROOTFS_NAME, ROOTFS_VERITY_DEVICE,
            ROOTFS_VERITY_NAME, VOLUME_GROUP_NAME,
        },
    },
    config::{DeltaBackend, DeltaLocation},
};
use block_devs::BlckExt;
use cryptpilot::{
    fs::cmd::CheckCommandOutput,
    provider::{IntoProvider as _, KeyProvider as _, VolumeType},
    types::{IntegrityType, MakeFsType},
};

const CRYPTPILOT_LVM_SYSTEM_DIR: &str = "/usr/lib/cryptpilot/lvm/";

/// dm-snapshot chunk size in 512-byte sectors, shared by the snapshot table
/// and the COW header initialization: the persistent snapshot driver reads
/// and writes whole chunks, so any region the guest initializes on the COW
/// device must cover a full chunk or the read fails (with dm-integrity,
/// sectors never written through the mapping have no valid tag).
const SNAPSHOT_CHUNK_SIZE_SECTORS: u64 = 16;

const SNAPSHOT_CHUNK_SIZE_BYTES: usize = SNAPSHOT_CHUNK_SIZE_SECTORS as usize * 512;

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

    // Load required kernel modules for LVM and device mapper
    cryptpilot::fs::kernel_module::ensure_module_loaded("dm_mod", &[]).await;

    // 1. Checking and activating LVM volume group
    tracing::info!(
        volume_group_name = VOLUME_GROUP_NAME,
        "[ 1/4 ] Checking and activating LVM volume group"
    );
    Command::new("vgchange")
        .args(["-a", "y", VOLUME_GROUP_NAME])
        .run()
        .await
        .with_context(|| format!("Failed to activate LVM volume group '{VOLUME_GROUP_NAME}'"))?;

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

    let backend = fde_config.rootfs.delta_backend.unwrap_or_default();

    let (dm_verity_output_name, dm_verity_output_device) = match backend {
        DeltaBackend::Overlayfs => (ROOTFS_NAME, Path::new(ROOTFS_DEVICE)),
        DeltaBackend::DmSnapshot => (ROOTFS_VERITY_NAME, Path::new(ROOTFS_VERITY_DEVICE)),
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

    // 4. Setup delta volume and overlay backend
    {
        let delta_location = fde_config
            .rootfs
            .delta_location
            .unwrap_or(DeltaLocation::Disk);

        tracing::info!(
            ?backend,
            ?delta_location,
            "[ 4/4 ] Setting up delta volume if required"
        );

        if matches!(
            delta_location,
            DeltaLocation::Disk | DeltaLocation::DiskPersist
        ) {
            tracing::info!("Expanding system PV partition");
            if let Err(error) = expand_system_pv_partition().await {
                tracing::warn!(?error, "Failed to expend the system PV partition");
            }

            // Ensure delta logical volume exists
            ensure_delta_volume_exist_and_expanded().await?;

            let (recreate, integrity) =
                setup_delta_volume_luks2(&fde_config.delta, delta_location).await?;

            // Setup delta volume based on backend type
            match backend {
                DeltaBackend::Overlayfs => {
                    let delta_device = Path::new(DELTA_DEVICE);
                    if recreate {
                        tracing::info!("Creating ext4 fs on delta volume");
                        cryptpilot::fs::mkfs::force_mkfs(
                            delta_device,
                            &MakeFsType::Ext4,
                            integrity,
                        )
                        .await?;
                    } else {
                        // Resize existing filesystem to fill the expanded device
                        resize_ext4_filesystem(delta_device).await?;
                    }
                }
                DeltaBackend::DmSnapshot => {
                    // Build dm-snapshot device chain
                    setup_dm_snapshot_device_chain(
                        dm_verity_output_device,
                        Path::new(DELTA_DEVICE),
                        matches!(delta_location, DeltaLocation::DiskPersist),
                        recreate,
                    )
                    .await?;

                    // Resize rootfs filesystem to fill the expanded device after building snapshot chain
                    resize_ext4_filesystem(Path::new(ROOTFS_DEVICE)).await?;
                }
            }

            // Mark the delta volume as initialized after format + mkfs.
            // format() always sets subsystem="cryptpilot-initializing";
            // this transitions it to "cryptpilot" (Ready) for consistency,
            // regardless of delta_location or provider type.
            if recreate {
                cryptpilot::fs::luks2::mark_volume_as_initialized(Path::new(DELTA_LOGICAL_VOLUME))
                    .await?;
            }
        } else {
            // No need to set up delta volume
            match backend {
                DeltaBackend::Overlayfs => {
                    // Nothing to do
                }
                DeltaBackend::DmSnapshot => {
                    tracing::info!("Creating zram device for COW storage");
                    let cow_device = create_zram_cow_device().await?;
                    // Build dm-snapshot device chain
                    setup_dm_snapshot_device_chain(
                        dm_verity_output_device,
                        &cow_device,
                        false,
                        false,
                    )
                    .await?;
                    // Resize rootfs filesystem to fill the expanded device after building snapshot chain
                    resize_ext4_filesystem(Path::new(ROOTFS_DEVICE)).await?;
                }
            }
        }
    }

    tracing::info!("Both rootfs volume and delta volume are ready");

    Ok(())
}

async fn ensure_delta_volume_exist_and_expanded() -> Result<(), anyhow::Error> {
    if !Path::new(DELTA_LOGICAL_VOLUME).exists() {
        tracing::info!(
            "Delta logical volume does not exist, assume it is first time boot and create it."
        );

        // Due to there is no udev in initrd, the lvcreate will complain that /dev/cryptpilot/delta not exist. A workaround is to set '--zero n' and zeroing the first 4k of logical volume manually.
        // See https://serverfault.com/a/1059400
        async {
            Command::new("lvcreate")
                .args([
                    "-n",
                    DELTA_NAME,
                    "--zero",
                    "n",
                    "-l",
                    "100%FREE",
                    "cryptpilot",
                ])
                .env("LVM_SYSTEM_DIR", CRYPTPILOT_LVM_SYSTEM_DIR)
                .run()
                .await?;
            File::options()
                .write(true)
                .open(DELTA_LOGICAL_VOLUME)
                .await?
                .write_all(&[0u8; 4096])
                .await?;
            Ok::<_, anyhow::Error>(())
        }
        .await
        .context("Failed to create delta logical volume")?;
    } else {
        tracing::info!("Expanding delta logical volume");
        if let Err(error) = expand_system_delta_lv().await {
            tracing::warn!(?error, "Failed to expend delta logical volume");
        }
    }
    Ok(())
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
        cryptpilot::fs::kernel_module::ensure_module_loaded("dm-verity", &[]).await;

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

VG_NAME="cryptpilot"

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
    # the growpart command fill also call lvm pvresize to resize the related delta volume
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

async fn expand_system_delta_lv() -> Result<()> {
    Command::new("lvextend")
        .arg("-l")
        .arg("+100%FREE")
        .arg(DELTA_LOGICAL_VOLUME)
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

/// Setup delta volume LUKS2 encryption and return whether content should be recreated
async fn setup_delta_volume_luks2(
    delta_config: &crate::config::DeltaConfig,
    delta_location: DeltaLocation,
) -> Result<(bool, IntegrityType)> {
    tracing::info!("Fetching passphrase for delta volume");
    let provider = delta_config.encrypt.key_provider.clone().into_provider();
    let passphrase = provider
        .get_key()
        .await
        .context("Failed to get passphrase")?;

    let integrity = if delta_config.integrity {
        IntegrityType::Journal // Select Journal mode since it is persistent storage
    } else {
        IntegrityType::None
    };

    let delta_logical_volume_dev = Path::new(DELTA_LOGICAL_VOLUME);

    let recreate = if matches!(provider.volume_type(), VolumeType::Temporary) {
        tracing::info!("Key provider is temporary, will recreate delta volume content");
        true
    } else if !cryptpilot::fs::luks2::is_initialized(delta_logical_volume_dev).await? {
        tracing::info!("Delta volume is not initialized, will create new content");
        true
    } else if matches!(delta_location, DeltaLocation::Disk) {
        tracing::info!("Overlay type is disk (non-persistent), will recreate delta volume content");
        true
    } else {
        tracing::info!("Delta volume is initialized and overlay type is persistent, will reuse existing content");
        false
    };

    if recreate {
        // Create a LUKS volume on it
        tracing::info!("Creating LUKS2 on delta volume");
        cryptpilot::fs::luks2::format(delta_logical_volume_dev, &passphrase, integrity).await?;
    }

    // TODO: support change size of the LUKS2 volume and inner ext4 file system
    tracing::info!("Opening delta volume");
    cryptpilot::fs::luks2::open_with_check_passphrase(
        DELTA_NAME,
        delta_logical_volume_dev,
        &passphrase,
        integrity,
    )
    .await?;

    Ok((recreate, integrity))
}

async fn create_zram_cow_device() -> Result<PathBuf> {
    // Load zram module if not available
    cryptpilot::fs::kernel_module::ensure_module_loaded("zram", &[]).await;

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

/// Zero the COW header region through the device mapping.
///
/// Must cover a full snapshot chunk: the persistent snapshot driver reads
/// and writes whole chunks, and with dm-integrity only sectors written
/// through the mapping carry a valid tag.
async fn wipe_cow_device_header_async(device_path: &Path) -> Result<(), anyhow::Error> {
    use tokio::fs::OpenOptions;

    let mut file = OpenOptions::new()
        .write(true)
        .open(device_path)
        .await
        .context("Failed to open device for wiping")?;

    file.write_all(&vec![0u8; SNAPSHOT_CHUNK_SIZE_BYTES])
        .await?;
    file.sync_all().await?;
    Ok(())
}

/// Wipe the COW header chunk only when the header region reads back blank.
///
/// blkid reports "no signatures" both for a genuinely blank device and for
/// one whose probe reads fail (dm-integrity rejects sectors whose tags were
/// never written, e.g. the end-of-device probe), so "no signatures" alone
/// is not proof of blank. The header chunk is: unreadable means we cannot
/// tell (fail closed), non-zero content is left untouched for the snapshot
/// to interpret (valid "SnAp" metadata gets reused; anything else makes
/// dmsetup fail closed), all zeros mean blank and safe to initialize.
async fn wipe_cow_header_after_read_check(device_path: &Path) -> Result<(), anyhow::Error> {
    use tokio::fs::OpenOptions;
    use tokio::io::AsyncReadExt;

    let mut file = OpenOptions::new()
        .read(true)
        .open(device_path)
        .await
        .context("Failed to open COW device for header check")?;

    let mut header = vec![0u8; SNAPSHOT_CHUNK_SIZE_BYTES];
    file.read_exact(&mut header).await.context(
        "COW header chunk is unreadable, cannot tell whether the volume is blank; refusing to wipe",
    )?;
    drop(file);

    if header.iter().any(|&b| b != 0) {
        tracing::info!(
            "COW header has existing content, keeping it for the snapshot to reuse \
             (blkid misreported the device as clean)"
        );
        return Ok(());
    }

    wipe_cow_device_header_async(device_path).await
}

async fn setup_dm_snapshot_device_chain(
    rootfs_device: &Path,
    cow_device: &Path,
    persistent: bool,
    fresh_delta: bool,
) -> Result<()> {
    tracing::info!(
        ?rootfs_device,
        ?cow_device,
        persistent,
        "Building dm-snapshot device chain"
    );

    // Load required kernel modules
    cryptpilot::fs::kernel_module::ensure_module_loaded("dm-snapshot", &[]).await;
    cryptpilot::fs::kernel_module::ensure_module_loaded("dm-zero", &[]).await;

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

    if !persistent {
        // Non-persistent mode: directly wipe the COW device, no need to probe
        tracing::info!("Non-persistent mode: wiping COW device");
        wipe_cow_device_header_async(cow_device)
            .await
            .context("Failed to wipe COW device")?;
    } else if fresh_delta {
        // The COW device was created on this boot and is blank by
        // construction; unwritten sectors fail integrity reads, so probing
        // here would report "no signatures" for the wrong reason. Initialize
        // the header chunk directly.
        tracing::info!("Persistent mode: fresh delta, initializing COW header");
        wipe_cow_device_header_async(cow_device)
            .await
            .context("Failed to wipe COW device")?;
    } else {
        // Persistent mode: probe COW device to determine safe action
        tracing::info!("Persistent mode: probing COW device state");
        let probe = cryptpilot::fs::blkid::probe_device(cow_device).await?;

        match probe {
            cryptpilot::fs::blkid::BlkidProbeResult::NoSignatures => {
                // blkid's "no signatures" also covers probe reads that
                // failed, so the header chunk itself decides: wiped only
                // when blank, kept when it holds snapshot metadata.
                tracing::info!("COW device reports clean, verifying header");
                wipe_cow_header_after_read_check(cow_device)
                    .await
                    .context("Failed to wipe COW device")?;
            }
            cryptpilot::fs::blkid::BlkidProbeResult::KnownSignature { .. }
                if probe.is_dm_snapshot_cow() =>
            {
                // No need to wipe and just use the metadata header
                tracing::info!("COW device has dm-snapshot metadata, using it");
            }
            cryptpilot::fs::blkid::BlkidProbeResult::KnownSignature {
                fs_type,
                pt_type,
                subsystem,
            } => {
                // Some other filesystem/partition signature detected — protect user data
                bail!(
                    "COW device has valuable data (fs_type={fs_type:?}, pt_type={pt_type:?}, subsystem={subsystem:?}), \
                     cannot overwrite in persistent mode"
                );
            }
        }
    }

    // Create dm-snapshot device
    tracing::info!("Creating dm-snapshot device");
    Command::new("dmsetup")
        .arg("create")
        .arg(ROOTFS_NAME)
        .arg("--table")
        .arg(format!(
            "0 {} snapshot {} {} {} {}",
            linear_size,
            ROOTFS_EXTENDED_DEVICE,
            cow_device.to_string_lossy(),
            if persistent { "PO" } else { "N" },
            SNAPSHOT_CHUNK_SIZE_SECTORS
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

    // Offline resize requires a check after the last mount, even on a clean
    // filesystem. This must happen here, before the initrd mounts the device.
    Command::new("e2fsck")
        .args(["-f", "-p"])
        .arg(device)
        .run_with_status_checker(|code, _, _| match code {
            0 | 1 => Ok(()), // Clean, or errors corrected without requiring a reboot.
            _ => bail!("Filesystem check failed with exit code {code}"),
        })
        .await
        .context(format!(
            "Failed to check ext4 filesystem on {} before resizing",
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

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_SIZE: u64 = 64 * 1024 * 1024;
    const SENTINEL: &[u8] = b"persistent rootfs data\n";

    async fn debugfs(image: &Path, command: &str) -> Result<Vec<u8>> {
        Command::new("debugfs")
            .args(["-w", "-R", command])
            .arg(image)
            .run()
            .await
    }

    async fn ext4_image() -> Result<(tempfile::TempDir, PathBuf)> {
        let dir = tempfile::tempdir()?;
        let image = dir.path().join("ext4.img");
        File::create(&image).await?.set_len(IMAGE_SIZE).await?;
        Command::new("mkfs.ext4")
            .args(["-q", "-F", "-b", "4096"])
            .arg(&image)
            .run()
            .await?;
        let data = dir.path().join("data");
        tokio::fs::write(&data, SENTINEL).await?;
        debugfs(&image, &format!("write {} sentinel", data.display())).await?;
        Ok((dir, image))
    }

    async fn assert_sentinel(image: &Path) -> Result<()> {
        assert_eq!(debugfs(image, "cat /sentinel").await?, SENTINEL);
        Ok(())
    }

    #[tokio::test]
    async fn test_wipe_cow_header_covers_full_snapshot_chunk() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cow = dir.path().join("cow.img");
        // Non-zero pattern makes a partial wipe visible.
        tokio::fs::write(&cow, vec![0xAAu8; SNAPSHOT_CHUNK_SIZE_BYTES * 2]).await?;

        wipe_cow_device_header_async(&cow).await?;

        let data = tokio::fs::read(&cow).await?;
        assert!(
            data[..SNAPSHOT_CHUNK_SIZE_BYTES].iter().all(|&b| b == 0),
            "wipe must zero the whole first chunk the snapshot driver reads"
        );
        assert!(data[SNAPSHOT_CHUNK_SIZE_BYTES..].iter().all(|&b| b == 0xAA));
        Ok(())
    }

    #[tokio::test]
    async fn test_guarded_wipe_requires_readable_header_chunk() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cow = dir.path().join("short.img");
        // Shorter than one chunk: reading the header region fails, so the
        // wipe must refuse rather than treat the failure as a blank device.
        tokio::fs::write(&cow, vec![0xAAu8; SNAPSHOT_CHUNK_SIZE_BYTES / 2]).await?;

        let result = wipe_cow_header_after_read_check(&cow).await;

        assert!(result.is_err(), "unreadable header must abort the wipe");
        let data = tokio::fs::read(&cow).await?;
        assert!(
            data.iter().all(|&b| b == 0xAA),
            "refused wipe must leave the device untouched"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_guarded_wipe_preserves_nonzero_header() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cow = dir.path().join("snapcow.img");
        // dm-snapshot persistent metadata starts with the "SnAp" magic.
        // blkid misreports such a device as clean when its probe reads fail
        // under dm-integrity, so the wipe must verify the header content
        // itself and keep anything non-zero for the snapshot to interpret.
        let mut header = vec![0u8; SNAPSHOT_CHUNK_SIZE_BYTES];
        header[0..4].copy_from_slice(b"SnAp");
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&(SNAPSHOT_CHUNK_SIZE_SECTORS as u32).to_le_bytes());
        tokio::fs::write(&cow, &header).await?;

        wipe_cow_header_after_read_check(&cow).await?;

        let data = tokio::fs::read(&cow).await?;
        assert_eq!(
            data, header,
            "non-zero header holds dm-snapshot metadata; wiping it would destroy the previous boot's delta"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_resize_clean_ext4_checked_before_last_mount() -> Result<()> {
        let (_dir, image) = ext4_image().await?;
        // Both timestamps are in the past; a clean state alone does not satisfy
        // resize2fs's requirement to check the filesystem after its last mount.
        debugfs(&image, "set_super_value lastcheck @1000000000").await?;
        debugfs(&image, "set_super_value mtime @1000000060").await?;
        Command::new("tune2fs")
            .args(["-O", "read-only"])
            .arg(&image)
            .run()
            .await?;

        resize_ext4_filesystem(&image).await?;
        assert_sentinel(&image).await?;
        // Rechecking a filesystem that already fills its device must also work.
        resize_ext4_filesystem(&image).await?;
        assert_sentinel(&image).await
    }

    #[tokio::test]
    async fn test_resize_ext4_after_repair_and_device_growth() -> Result<()> {
        let (_dir, image) = ext4_image().await?;
        // A wrong free-block count is safely repaired by preen (exit status 1).
        debugfs(&image, "set_super_value free_blocks_count 0").await?;
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(&image)
            .await?
            .set_len(IMAGE_SIZE * 2)
            .await?;

        resize_ext4_filesystem(&image).await?;
        assert_sentinel(&image).await?;
        let stats = debugfs(&image, "stats").await?;
        let stats = String::from_utf8(stats)?;
        let count = stats
            .lines()
            .find_map(|line| line.strip_prefix("Block count:"))
            .context("Missing ext4 block count")?
            .trim()
            .parse::<u64>()?;
        assert_eq!(count * 4096, IMAGE_SIZE * 2);
        Ok(())
    }

    #[tokio::test]
    async fn test_resize_ext4_stops_on_uncorrected_errors() -> Result<()> {
        let (dir, image) = ext4_image().await?;
        debugfs(
            &image,
            &format!("write {} duplicate", dir.path().join("data").display()),
        )
        .await?;
        let blocks = debugfs(&image, "blocks /sentinel").await?;
        let block = String::from_utf8(blocks)?
            .split_whitespace()
            .next()
            .context("Missing sentinel data block")?
            .parse::<u64>()?;
        // Duplicate data blocks require intervention; preen must not let the
        // offline resize proceed with this inconsistent filesystem.
        debugfs(
            &image,
            &format!("set_inode_field /duplicate block[0] {block}"),
        )
        .await?;

        let error = resize_ext4_filesystem(&image).await.unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("Failed to check ext4 filesystem"), "{error}");
        assert!(error.contains("exit code: 4"), "{error}");
        assert_sentinel(&image).await
    }
}
