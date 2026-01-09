# Quick Start Guide: cryptpilot-crypt

This guide walks you through setting up encrypted data volumes for runtime use.

## Prerequisites

- cryptpilot-crypt installed on your system
- A block device or partition to encrypt (e.g., `/dev/nvme1n1p1`)
- The device should be unmounted and not in use

## Example: Setting Up Encrypted Data Partitions

In this example, we will create encrypted volumes with different configurations. You'll need an empty disk (e.g., `/dev/nvme1n1`) for this example.

### Step 1: Create Partition

Create a GPT partition table with one primary partition:

```sh
parted --script /dev/nvme1n1 \
    mktable gpt \
    mkpart part1 0% 100%
```

### Step 2: Create Volume Configuration

Create a configuration file at `/etc/cryptpilot/volumes/data0.toml`:

```sh
mkdir -p /etc/cryptpilot/volumes
cat << EOF > /etc/cryptpilot/volumes/data0.toml
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.otp]
EOF
```

**Configuration Explanation:**

- `volume = "data0"`: Volume name (will create `/dev/mapper/data0`)
- `dev = "/dev/nvme1n1p1"`: Underlying block device
- `auto_open = true`: Automatically open at boot
- `makefs = "ext4"`: Create ext4 filesystem on first initialization  
- `integrity = true`: Enable dm-integrity for data authenticity
- `[encrypt.otp]`: Use One-Time Password (data is volatile)

> [!WARNING]
> This volume will be encrypted with One-Time Password, which means the data on it is **volatile** and will be lost after closing. The volume will be automatically opened during system startup.

### Step 3: Check Configuration

Validate the configuration:

```sh
cryptpilot-crypt config check --keep-checking
```

### Step 4: Open the Volume

Open (decrypt) the volume:

```sh
cryptpilot-crypt open data0
```

This will initialize the volume on first run (format with LUKS2, create filesystem, set up dm-integrity if enabled).

### Step 5: Check Volume Status

Verify the volume is opened:

```sh
cryptpilot-crypt show
```

Example output:

```
╭────────┬───────────────────┬─────────────────┬──────────────┬──────────────────┬──────────────┬────────╮
│ Volume ┆ Volume Path       ┆ Underlay Device ┆ Key Provider ┆ Extra Options    ┆ Initialized  ┆ Opened │
╞════════╪═══════════════════╪═════════════════╪══════════════╪══════════════════╪══════════════╪════════╡
│ data0  ┆ /dev/mapper/data0 ┆ /dev/nvme1n1p1  ┆ otp          ┆ auto_open = true ┆ Not Required ┆ True   │
│        ┆                   ┆                 ┆              ┆ makefs = "ext4"  ┆              ┆        │
│        ┆                   ┆                 ┆              ┆ integrity = true ┆              ┆        │
╰────────┴───────────────────┴─────────────────┴──────────────┴──────────────────┴──────────────┴────────╯
```

### Step 6: Mount and Use

Mount the volume and start using it:

```sh
mkdir -p /mnt/data0
mount /dev/mapper/data0 /mnt/data0
```

Now you can read and write files in `/mnt/data0`.

### Step 7: Setup Auto-Open at Boot

If you want to automatically open the volume during system startup:

1. Ensure `auto_open = true` is set in the volume configuration (already done in Step 2)

2. Enable the systemd service:

```sh
systemctl enable --now cryptpilot.service
```

The volume will now be automatically opened at boot.

### Step 8: Close the Volume

When done, unmount and close:

```sh
umount /mnt/data0
cryptpilot-crypt close data0
```

> [!WARNING]
> With OTP provider, closing the volume will permanently erase all data! OTP is for temporary/scratch storage only.

## Additional Examples

### Persistent Storage with KBS (Production)

For production workloads, use Key Broker Service with remote attestation:

```sh
cat << EOF > /etc/cryptpilot/volumes/data1.toml
volume = "data1"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data1-key"
EOF

cryptpilot-crypt open data1
mount /dev/mapper/data1 /mnt/data1
```

### Cloud-Managed Keys with KMS

For Alibaba Cloud users:

```sh
cat << EOF > /etc/cryptpilot/volumes/data2.toml
volume = "data2"
dev = "/dev/nvme1n1p3"
auto_open = true
makefs = "xfs"

[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
EOF

cryptpilot-crypt open data2
mount /dev/mapper/data2 /mnt/data2
```

### Multiple Volumes with Different Providers

You can configure multiple volumes:

```sh
# Temporary storage (OTP)
cat > /etc/cryptpilot/volumes/scratch.toml << EOF
volume = "scratch"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.otp]
EOF

# Persistent data (KBS)
cat > /etc/cryptpilot/volumes/data.toml << EOF
volume = "data"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
EOF

# Enable auto-open
systemctl enable --now cryptpilot.service
```

### Swap Partition Encryption

Create an encrypted swap partition:

```sh
cat > /etc/cryptpilot/volumes/swap.toml << EOF
volume = "swap"
dev = "/dev/nvme1n1p4"
auto_open = true
makefs = "swap"

[encrypt.otp]
EOF

cryptpilot-crypt open swap
swapon /dev/mapper/swap
echo "/dev/mapper/swap none swap defaults 0 0" >> /etc/fstab
```

## Troubleshooting

### Configuration Check Failed

If `config check` reports errors:

```sh
cryptpilot-crypt config check --keep-checking
```

Common issues:
- Missing required fields (`volume`, `dev`, `encrypt`)
- Invalid device path
- Invalid key provider configuration

### Initialization Failed

If `cryptpilot-crypt init` fails:

1. **Check device exists**: `ls -l /dev/nvme1n1p1`
2. **Check device is not in use**: `lsblk`, `mount | grep nvme1n1p1`
3. **Check permissions**: Run with sufficient privileges
4. **Check key provider**: Ensure provider is reachable (KBS/KMS)

### Open Failed

If `cryptpilot-crypt open` fails:

1. **Check volume is initialized**: `cryptpilot-crypt show`
2. **Check key provider**: Verify network/attestation is working
3. **Check device**: Ensure underlying device is available
4. **Check logs**: `journalctl -u cryptpilot.service`

### Auto-Open Not Working

If volumes don't open at boot:

1. **Check service is enabled**: `systemctl status cryptpilot.service`
2. **Check auto_open setting**: Verify `auto_open = true` in config
3. **Check service logs**: `journalctl -u cryptpilot.service`
4. **Check network**: For remote providers (KBS/KMS), ensure network is up

## Next Steps

- [Configuration Guide](configuration.md) - Detailed configuration options
- [Systemd Service](systemd-service.md) - Auto-open volumes at boot
- [Key Providers](../../docs/key-providers.md) - Key provider configuration details
