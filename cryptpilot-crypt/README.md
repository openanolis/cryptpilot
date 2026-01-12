# cryptpilot-crypt: Runtime Volume Encryption

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

`cryptpilot-crypt` provides runtime encryption for data volumes in confidential computing environments. It manages encrypted LUKS2 volumes with flexible key management and automatic mounting.

## Features

- **Volume Encryption**: Encrypt individual data volumes with LUKS2
- **Multiple Key Providers**: KBS, KMS, OIDC, TPM2, Exec, OTP
- **Auto-Open**: Automatically decrypt and mount volumes at boot
- **Integrity Protection**: Optional dm-integrity for data authenticity
- **Flexible File Systems**: Support for ext4, xfs, vfat, swap

## Installation

Install from the [latest release](https://github.com/openanolis/cryptpilot/releases):

```sh
# Install cryptpilot-crypt package
rpm --install cryptpilot-crypt-*.rpm
```

Or build from source (see [Development Guide](../docs/development.md)).

## Quick Start

Encrypt a data volume:

```sh
# Create configuration
cat > /etc/cryptpilot/volumes/data0.toml << EOF
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.otp]
EOF

# Initialize and open
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [Detailed Quick Start Guide](docs/quick-start.md)

## Configuration

Configuration files are located in `/etc/cryptpilot/volumes/`:

- Each `.toml` file defines one volume
- File name can be arbitrary (e.g., `data0.toml`, `backup.toml`)

See [Configuration Guide](docs/configuration.md) for detailed options.

### Configuration Example Templates

- [otp.toml.template](../dist/etc/volumes/otp.toml.template) - One-time password (volatile)
- [kbs.toml.template](../dist/etc/volumes/kbs.toml.template) - Key Broker Service
- [kms.toml.template](../dist/etc/volumes/kms.toml.template) - Alibaba Cloud KMS
- [oidc.toml.template](../dist/etc/volumes/oidc.toml.template) - KMS with OIDC
- [exec.toml.template](../dist/etc/volumes/exec.toml.template) - Custom executable

## Commands

### `cryptpilot-crypt show`

Display status of all configured volumes:

```sh
cryptpilot-crypt show [volume-name...] [--json]
```

Options:
- `volume-name`: Optional volume name(s) to show. If not specified, show all volumes.
- `--json`: Output as JSON format instead of table

Examples:
```sh
# Show all volumes
cryptpilot-crypt show

# Show specific volume(s)
cryptpilot-crypt show data0
cryptpilot-crypt show data0 data1

# Output as JSON
cryptpilot-crypt show --json
cryptpilot-crypt show data0 --json
```

Example table output:
```
╭────────┬───────────────────┬─────────────────┬──────────────┬──────────────────┬──────────────┬────────╮
│ Volume ┆ Volume Path       ┆ Underlay Device ┆ Key Provider ┆ Extra Options    ┆ Initialized  ┆ Opened │
╞════════╪═══════════════════╪═════════════════╪══════════════╪══════════════════╪══════════════╪════════╡
│ data0  ┆ /dev/mapper/data0 ┆ /dev/nvme1n1p1  ┆ otp          ┆ auto_open = true ┆ Not Required ┆ True   │
│        ┆                   ┆                 ┆              ┆ makefs = "ext4"  ┆              ┆        │
│        ┆                   ┆                 ┆              ┆ integrity = true ┆              ┆        │
╰────────┴───────────────────┴─────────────────┴──────────────┴──────────────────┴──────────────┴────────╯
```

Example JSON output:
```json
[
  {
    "volume": "data0",
    "volume_path": "/dev/mapper/data0",
    "underlay_device": "/dev/nvme1n1p1",
    "device_exists": true,
    "key_provider": "otp",
    "extra_options": {
      "auto_open": true,
      "makefs": "ext4",
      "integrity": true
    },
    "needs_initialize": false,
    "initialized": true,
    "opened": true
  }
]
```

JSON output fields:
- `volume`: Volume name
- `volume_path`: Path to the decrypted volume (always shows the mapper path)
- `underlay_device`: Underlying encrypted block device path
- `device_exists`: Whether the underlying device exists
- `key_provider`: Key provider type (e.g., `otp`, `kbs`, `kms`, `oidc`, `exec`)
- `extra_options`: Additional volume configuration (`null` if serialization fails)
- `needs_initialize`: Whether the volume needs initialization (false for temporary volumes like OTP, true for persistent volumes)
- `initialized`: Whether LUKS2 is initialized (false if device doesn't exist or initialization check fails, true if device exists and volume doesn't need initialization, or actual initialization status for persistent volumes)
- `opened`: Whether the volume is currently opened/decrypted

### `cryptpilot-crypt init`

Initialize a new encrypted volume:

```sh
cryptpilot-crypt init <volume-name>
```

Options:
- `--skip-check-passphrase`: Skip passphrase validation

### `cryptpilot-crypt open`

Open (decrypt) an encrypted volume:

```sh
cryptpilot-crypt open <volume-name>
```

Options:
- `--skip-check-passphrase`: Skip passphrase validation

### `cryptpilot-crypt close`

Close (unmount and lock) a volume:

```sh
cryptpilot-crypt close <volume-name>
```

### `cryptpilot-crypt config check`

Validate volume configurations:

```sh
cryptpilot-crypt config check [--keep-checking] [--skip-check-passphrase]
```

Options:
- `--keep-checking`: Continue checking all volumes even if errors found
- `--skip-check-passphrase`: Skip passphrase validation

## Volume Configuration Options

Each volume configuration supports:

- **`volume`** (required): Volume name (used as `/dev/mapper/<volume>`)
- **`dev`** (required): Underlying block device path
- **`auto_open`** (optional, default: false): Auto-decrypt at boot
- **`makefs`** (optional): File system type (`ext4`, `xfs`, `vfat`, `swap`)
- **`integrity`** (optional, default: false): Enable dm-integrity
- **`encrypt`** (required): Key provider configuration

See [Configuration Guide](docs/configuration.md) for details.

## Key Providers

Supports multiple key providers:

- **OTP**: One-time password (volatile, regenerated each open)
- **KBS**: Key Broker Service with remote attestation
- **KMS**: Alibaba Cloud KMS with Access Key authentication
- **OIDC**: KMS with OpenID Connect authentication
- **Exec**: Custom executable providing keys

See [Key Providers](../docs/key-providers.md) for detailed configuration.

## Documentation

- [Quick Start Guide](docs/quick-start.md) - Step-by-step examples
- [Configuration Guide](docs/configuration.md) - Detailed configuration options
- [Systemd Service](docs/systemd-service.md) - Auto-open volumes at boot
- [Development Guide](../docs/development.md) - Build and test instructions

## Use Cases

### Temporary/Volatile Storage (OTP)

Use OTP provider for scratch space that's wiped on each reboot:

```toml
[encrypt.otp]
```

### Persistent Encrypted Storage (KBS)

Use KBS for production workloads with attestation:

```toml
[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/volume-key"
```

### Cloud-Managed Keys (KMS)

Use Alibaba Cloud KMS for centralized key management:

```toml
[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

## Integration with /etc/fstab

After opening a volume, add to `/etc/fstab` for automatic mounting:

```sh
echo "/dev/mapper/data0 /mnt/data0 ext4 defaults 0 2" >> /etc/fstab
```

Combined with `auto_open = true`, volumes will be decrypted and mounted automatically.

## Supported Distributions

- [Anolis OS 23](https://openanolis.cn/anolisos/23)
- [Alibaba Cloud Linux 3](https://www.aliyun.com/product/alinux)

## License

Apache-2.0

## See Also

- [cryptpilot-fde](../cryptpilot-fde/) - Full disk encryption
- [cryptpilot-verity](../cryptpilot-verity/) - dm-verity utilities
- [Main Project README](../README.md)
