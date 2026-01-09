# cryptpilot: Confidentiality for OS Booting and Data at Rest in TEEOS

[![Building](/../../actions/workflows/build-rpm.yml/badge.svg)](/../../actions/workflows/build-rpm.yml)
![GitHub Release](https://img.shields.io/github/v/release/openanolis/cryptpilot)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

cryptpilot provides comprehensive encryption solutions for confidential computing environments, protecting both system boot integrity and data at rest.

## Project Structure

cryptpilot is split into specialized packages:

### [cryptpilot-fde](cryptpilot-fde/)

**Full Disk Encryption** - Encrypts entire system disks with boot integrity protection.

- Encrypts rootfs and data partitions
- dm-verity integrity protection
- Remote attestation and measurement for secure key retrieval
- Initrd integration for early boot decryption

**Quick Start:**
```sh
# Encrypt a disk image
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase MyPassword
```

📖 [Full Documentation](cryptpilot-fde/README.md) | [Quick Start Guide](cryptpilot-fde/docs/quick-start.md)

### [cryptpilot-crypt](cryptpilot-crypt/)

**Runtime Volume Encryption** - Manages encrypted data volumes during system runtime.

- LUKS2 volume encryption
- Auto-open at boot
- Multiple key providers (KBS, KMS, TPM2, etc.)
- Integrity protection with dm-integrity

**Quick Start:**
```sh
# Initialize and open a volume
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [Full Documentation](cryptpilot-crypt/README.md) | [Quick Start Guide](cryptpilot-crypt/docs/quick-start.md)

### [cryptpilot-verity](cryptpilot-verity/)

**Static Data Measurement** - Tools for computing and verifying hash values of static data.

## Features

- **Full Disk Encryption**: Protect entire system disks including rootfs
- **Volume Encryption**: Encrypt individual data partitions
- **Remote Attestation**: Measure and verify boot integrity
- **Flexible Key Management**: Support for KBS (remote attestation), KMS (Alibaba Cloud), OIDC (federated identity), and custom providers
- **Integrity Protection**: dm-verity and dm-integrity support
- **Auto-Mount**: Automatic decryption and mounting at boot

## Installation

### From Releases

Download from [latest release](https://github.com/openanolis/cryptpilot/releases):

```sh
# For full disk encryption
rpm --install cryptpilot-fde-*.rpm

# For runtime volume encryption
rpm --install cryptpilot-crypt-*.rpm

# (Optional) Main package for config directory
rpm --install cryptpilot-*.rpm
```

### From Source

Build RPM packages:

```sh
make create-tarball rpm-build
rpm --install /root/rpmbuild/RPMS/x86_64/cryptpilot-*.rpm
```

Or build DEB packages:

```sh
make create-tarball deb-build
dpkg -i /tmp/cryptpilot_*.deb
```

## Quick Examples

### Encrypt a VM Disk Image (FDE)

```sh
cryptpilot-convert --in ./source.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase MyPassword
```

📖 [Detailed FDE Examples](cryptpilot-fde/docs/quick-start.md)

### Encrypt Data Volumes (Crypt)

```sh
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [Detailed Crypt Examples](cryptpilot-crypt/docs/quick-start.md)

## Supported Distributions

- [Anolis OS 23](https://openanolis.cn/anolisos/23)
- [Alibaba Cloud Linux 3](https://www.aliyun.com/product/alinux)

## Documentation

### Package Documentation

- [cryptpilot-fde Documentation](cryptpilot-fde/README.md)
  - [FDE Configuration Guide](cryptpilot-fde/docs/configuration.md)
  - [Boot Process](cryptpilot-fde/docs/boot.md)
  - [cryptpilot-enhance](cryptpilot-fde/docs/cryptpilot_enhance.md)
  
- [cryptpilot-crypt Documentation](cryptpilot-crypt/README.md)
  - [Volume Configuration Guide](cryptpilot-crypt/docs/configuration.md)

### Development

- [Development Guide](docs/development.md) - Build, test, and package

## License

Apache-2.0

## Contributing

Contributions welcome! Please see [Development Guide](docs/development.md).

## See Also

- [Trustee Project](https://github.com/confidential-containers/trustee) - KBS and attestation services
- [Confidential Containers](https://github.com/confidential-containers) - Cloud-native confidential computing
