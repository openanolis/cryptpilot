use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::{fs::File, io::AsyncWriteExt, process::Command};

use crate::cmd::boot_service::{
    metadata::{load_metadata_from_file, Metadata},
    stage::{
        DATA_LAYER_NAME, DATA_LOGICAL_VOLUME, ROOTFS_DECRYPTED_LAYER_DEVICE,
        ROOTFS_DECRYPTED_LAYER_NAME, ROOTFS_HASH_LOGICAL_VOLUME, ROOTFS_LAYER_NAME,
        ROOTFS_LOGICAL_VOLUME,
    },
};
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
            ROOTFS_DECRYPTED_LAYER_NAME,
            ROOTFS_LOGICAL_VOLUME,
            &passphrase,
            IntegrityType::None,
        )
        .await?;
    } else {
        tracing::info!("Encryption is disabled for rootfs volume, skip setting up dm-crypt")
    }

    tracing::info!("Setting up dm-verity for rootfs volume");
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
    tracing::info!("[ 4/4 ] Setting up data volume");

    tracing::info!("Expanding system PV partition");
    if let Err(error) = expand_system_pv_partition().await {
        tracing::warn!(?error, "Failed to expend the system PV partition");
    }

    {
        // Check if the data logical volume exists
        let create_data_lv = !Path::new(DATA_LOGICAL_VOLUME).exists();
        if create_data_lv {
            tracing::info!(
                "Data logical volume does not exist, assume it is first time boot and create it."
            );

            // Due to there is no udev in initrd, the lvcreate will complain that /dev/system/data not exist. A workaround is to set '--zero n' and zeroing the first 4k of logical volume manually.
            // See https://serverfault.com/a/1059400
            async {
                Command::new("lvcreate")
                    .args(["-n", "data", "--zero", "n", "-l", "100%FREE", "system"])
                    .env("LVM_SYSTEM_DIR", CRYPTPILOT_LVM_SYSTEM_DIR)
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
        } else {
            tracing::info!("Expanding data logical volume");
            if let Err(error) = expand_system_data_lv().await {
                tracing::warn!(?error, "Failed to expend data logical volume");
            }
        }

        tracing::info!("Fetching passphrase for data volume");
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

        let recreate_data_lv_content =
            create_data_lv || matches!(provider.volume_type(), VolumeType::Temporary);
        if recreate_data_lv_content {
            // Create a LUKS volume on it
            tracing::info!("Creating LUKS2 on data volume");
            cryptpilot::fs::luks2::format(DATA_LOGICAL_VOLUME, &passphrase, integrity).await?;
        }

        // TODO: support change size of the LUKS2 volume and inner ext4 file system
        tracing::info!("Opening data volume");
        cryptpilot::fs::luks2::open_with_check_passphrase(
            DATA_LAYER_NAME,
            DATA_LOGICAL_VOLUME,
            &passphrase,
            integrity,
        )
        .await?;

        if recreate_data_lv_content {
            // Create a Ext4 fs on it
            tracing::info!("Creating ext4 fs on data volume");
            cryptpilot::fs::luks2::makefs_if_empty(DATA_LAYER_NAME, &MakeFsType::Ext4, integrity)
                .await?;
        }
    }

    tracing::info!("Both rootfs volume and data volume are ready");

    Ok(())
}

async fn load_metadata() -> Result<Metadata> {
    load_metadata_from_file(Path::new(METADATA_PATH_IN_INITRD)).await
}

async fn setup_rootfs_dm_verity(root_hash: &str, lower_dm_device: &str) -> Result<()> {
    async {
        Command::new("modprobe")
            .arg("dm-verity")
            .run()
            .await
            .context("Failed to load kernel module 'dm-verity'")?;

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
