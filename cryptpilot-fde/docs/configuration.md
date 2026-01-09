# FDE Configuration Guide

This guide covers configuration options for Full Disk Encryption (FDE) with cryptpilot-fde.

## Configuration Files Overview

The default configuration directory is `/etc/cryptpilot/`:

- **`global.toml`**: Global configuration (optional), see [global.toml.template](../../dist/etc/global.toml.template)
- **`fde.toml`**: FDE configuration for rootfs and data volumes

## FDE Configuration

System disk encryption (Full Disk Encryption) encrypts the entire system disk, providing protection for the root partition through encryption and integrity mechanisms. cryptpilot-fde also measures the root filesystem for remote attestation.

An encrypted system disk contains two main volumes:
- **Rootfs volume**: Read-only root filesystem
- **Data volume**: Writable data partition

### Configuration File Structure

Reference template: [fde.toml.template](../../dist/etc/fde.toml.template)

A basic FDE configuration must contain `[rootfs]` and `[data]` sections.

### Rootfs Volume Configuration

The rootfs volume stores the read-only root filesystem. Encryption is optional, but the volume is always protected by dm-verity and measured during boot.

An overlayfs layer provides write capability on top of the read-only rootfs.

**Configuration options:**

```toml
[rootfs]
# Storage location for the overlay layer: "disk" or "ram"
# - "disk": Stored on data volume (persistent across reboots)
# - "ram": Stored in memory (cleared on reboot)
rw_overlay = "disk"

# Encryption configuration (optional)
# If omitted, rootfs will not be encrypted (but still protected by dm-verity)
[rootfs.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/rootfs-key"
```

**Available fields:**

- **`rw_overlay`** (optional, default: `"disk"`): Overlay storage location
  - `"disk"`: Store on data volume (survives reboot)
  - `"ram"`: Store in tmpfs (cleared on reboot)

- **`encrypt`** (optional): Key provider configuration for rootfs encryption
  - If omitted, rootfs is not encrypted (but still integrity-protected)
  - See [Key Providers](../../docs/key-providers.md) for provider details

#### Measurement and Attestation

##### Measurement Principle

cryptpilot-fde uses Remote Attestation to measure the root filesystem:

1. Expected values are stored in initrd image
2. Initrd measurement is recorded in non-rewritable Event Log (CCEL)
3. dm-verity ensures root filesystem integrity
4. Event logs can be verified locally or remotely via attestation

##### Using KBS for Attestation

When using `kbs` as the key provider, measurement information is automatically included when fetching decryption keys from KBS. The KBS owner can configure [Remote Attestation Policies](https://github.com/openanolis/trustee/blob/main/attestation-service/docs/policy.md) to validate the measurements, establishing a full trust chain for confidential VM boot.

### Data Volume Configuration

The data volume uses the remaining disk space and contains an encrypted, writable filesystem. During boot, this volume is decrypted and mounted at `/data`.

**Configuration options:**

```toml
[data]
# Enable data integrity protection
integrity = true

# Encryption configuration (required)
[data.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
```

**Available fields:**

- **`integrity`** (optional, default: `false`): Enable dm-integrity for data authentication
  - When enabled, data is verified on every read
  - Prevents data tampering (but not replay attacks)

- **`encrypt`** (required): Key provider configuration for data volume encryption
  - See [Key Providers](../../docs/key-providers.md) for provider details

## Configuration Validation

Check configuration validity before use:

```sh
cryptpilot-fde -c /path/to/config config check --keep-checking
```

Options:
- `--keep-checking`: Continue checking all configurations even if errors found

## See Also

- [Key Providers](../../docs/key-providers.md) - Detailed key provider configuration
- [Boot Process](boot.md) - How cryptpilot-fde integrates with boot
- [cryptpilot-enhance](cryptpilot_enhance.md) - Disk hardening tool
- [Development Guide](../../docs/development.md) - Build and test instructions
