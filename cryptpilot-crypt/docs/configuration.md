# Volume Configuration Guide

This guide covers configuration options for data volume encryption with cryptpilot-crypt.

## Configuration Files Overview

The default configuration directory is `/etc/cryptpilot/volumes/`:

- Each `.toml` file defines one encrypted volume
- File names can be arbitrary (e.g., `data0.toml`, `backup.toml`)
- Files must have `.toml` extension

## What is a Volume?

In cryptpilot-crypt, a "volume" refers to any Linux block device (e.g., `/dev/nvme1n1p1`) that needs encryption. cryptpilot-crypt can initialize and manage encrypted volumes for storing confidential data.

**Main operations:**

- **Initialize** (`init`): Format the device as an encrypted LUKS2 volume (erases existing data)
- **Open** (`open`): Decrypt the volume and create `/dev/mapper/<volume-name>` device mapper
- **Close** (`close`): Lock the volume and remove device mapper

## Volume Configuration

Place volume configuration files in `/etc/cryptpilot/volumes/`:

Example: `/etc/cryptpilot/volumes/data0.toml`

### Configuration Template

Reference template: [otp.toml.template](../../dist/etc/volumes/otp.toml.template)

### Configuration Options

```toml
# The name of the resulting volume with decrypted data
volume = "data0"

# The path to the underlying encrypted device
dev = "/dev/nvme1n1p1"

# Whether to auto-open during boot (default: false)
auto_open = true

# File system to create during initialization
# Allowed values: "swap", "ext4", "xfs", "vfat"
# Skipped if device already has data
makefs = "ext4"

# Enable data integrity protection (default: false)
integrity = true

# Key provider configuration
[encrypt.otp]
```

**Field descriptions:**

- **`volume`** (required): Volume name used in `/dev/mapper/<volume>`
- **`dev`** (required): Path to the underlying block device
- **`auto_open`** (optional, default: `false`): Auto-decrypt during boot via systemd
- **`makefs`** (optional): File system type to create during initialization
  - Supported: `"swap"`, `"ext4"`, `"xfs"`, `"vfat"`
  - Skipped if device already contains data
- **`integrity`** (optional, default: `false`): Enable dm-integrity for data authentication
  - Verifies data on every read
  - Prevents tampering (but not replay attacks)
- **`encrypt`** (required): Key provider configuration (see [Key Providers](../../docs/key-providers.md))

## Auto-Open at Boot

To automatically decrypt and open volumes during system startup:

1. Set `auto_open = true` in volume configuration
2. Enable the systemd service:

```sh
systemctl enable --now cryptpilot.service
```

The service will automatically open all volumes with `auto_open = true`.

See [Systemd Service](systemd-service.md) for detailed information about the auto-open service.

## Usage Examples

### Example 1: Temporary Swap (OTP)

```toml
volume = "swap0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "swap"

[encrypt.otp]
```

Then add to `/etc/fstab`:
```
/dev/mapper/swap0 none swap defaults 0 0
```

### Example 2: Persistent Data (KBS)

```toml
volume = "data0"
dev = "/dev/nvme1n1p2"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data0-key"
```

Then add to `/etc/fstab`:
```
/dev/mapper/data0 /mnt/data0 ext4 defaults 0 2
```

### Example 3: Cloud-Managed Keys (KMS)

```toml
volume = "backup"
dev = "/dev/nvme1n1p3"
auto_open = false  # Manual open only
makefs = "xfs"

[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

Open manually when needed:
```sh
cryptpilot-crypt open backup
mount /dev/mapper/backup /mnt/backup
```

## Configuration Validation

Check configuration validity:

```sh
cryptpilot-crypt config check --keep-checking
```

Options:
- `--keep-checking`: Continue checking all volumes even if errors found
- `--skip-check-passphrase`: Skip passphrase validation (faster, less thorough)

## See Also

- [Key Providers](../../docs/key-providers.md) - Detailed key provider configuration
- [Systemd Service](systemd-service.md) - Auto-open volumes at boot
- [Development Guide](../../docs/development.md) - Build and test instructions
- [Main README](../README.md) - Quick start and usage examples
