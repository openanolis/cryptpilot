# Systemd Service for Auto-Open Volumes

cryptpilot-crypt provides a systemd service to automatically decrypt and open encrypted volumes during system startup.

## Overview

The `cryptpilot.service` systemd unit runs during the System Manager stage (after initrd has completed and the system has fully booted). It automatically processes all volumes configured with `auto_open = true` in their configuration files.

## Service Details

- **Service Unit**: `cryptpilot.service`
- **Location**: `/usr/lib/systemd/system/cryptpilot.service`
- **Execution Stage**: System Manager stage (after boot)
- **Command**: `/usr/bin/cryptpilot-crypt boot-service --stage system-volumes-auto-open`

## How It Works

During system startup, the service:

1. Scans all volume configuration files in `/etc/cryptpilot/volumes/`
2. Identifies volumes with `auto_open = true`
3. Attempts to open each volume using its configured key provider
4. Creates device mapper nodes at `/dev/mapper/<volume-name>`
5. Logs any errors encountered

## Enabling Auto-Open

To enable automatic opening of encrypted volumes at boot:

### 1. Configure Volumes

Ensure your volume configuration includes `auto_open = true`:

```toml
# /etc/cryptpilot/volumes/data0.toml
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data0-key"
```

### 2. Enable the Service

Enable and start the systemd service:

```sh
systemctl enable --now cryptpilot.service
```

This command:
- **`enable`**: Configures the service to start automatically at boot
- **`--now`**: Immediately starts the service (for the current session)

### 3. Verify Service Status

Check that the service is running:

```sh
systemctl status cryptpilot.service
```

Expected output:
```
● cryptpilot.service - Auto-open encrypted volumes
     Loaded: loaded (/usr/lib/systemd/system/cryptpilot.service; enabled; vendor preset: disabled)
     Active: active (exited) since ...
```

## Integration with /etc/fstab

After enabling auto-open, you can add entries to `/etc/fstab` for automatic mounting:

```sh
# /etc/fstab
/dev/mapper/data0  /mnt/data0  ext4  defaults  0  2
```

This achieves fully automated decryption and mounting:
1. `cryptpilot.service` opens the encrypted volume → `/dev/mapper/data0`
2. `systemd` mounts the device according to `/etc/fstab` → `/mnt/data0`

## Service Management

### Start the Service

```sh
systemctl start cryptpilot.service
```

### Stop the Service

```sh
systemctl stop cryptpilot.service
```

Note: Stopping the service does NOT close already-opened volumes. Use `cryptpilot-crypt close <volume>` to close volumes manually.

### Restart the Service

```sh
systemctl restart cryptpilot.service
```

### Disable Auto-Start

To prevent automatic opening at boot:

```sh
systemctl disable cryptpilot.service
```

## See Also

- [Configuration Guide](configuration.md) - Volume configuration options
- [Main README](../README.md) - Quick start and usage examples
- [Development Guide](../../docs/development.md) - Build and test instructions
