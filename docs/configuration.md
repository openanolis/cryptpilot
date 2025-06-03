# Configuration Instructions

Cryptpilot can configure encryption options through configuration files. The configuration file is in TOML format.

> For an explanation of TOML syntax, please refer to: https://toml.io/en/

## Overview of Configuration Files

The default Cryptpilot configuration directory is `/etc/cryptpilot/`, which mainly contains the following configuration files:

- `${config_dir}/global.toml`: Global configuration, please refer to the template [global.toml.template](/dist/etc/global.toml.template)

- `${config_dir}/fde.toml`: System disk encryption configuration, please refer to the [System Disk Encryption](#system-disk-encryption) section

- `${config_dir}/volumes/`: Directory storing data volume configurations, each configuration file corresponds to a data volume. Please refer to the [Data Disk Encryption](#data-disk-encryption) section


## Data Disk Encryption

### What is a "Volume"

In CryptPilot, a "volume" refers to any block device (e.g., /dev/nvme1n1p1) in Linux that needs to be encrypted. The CryptPilot tool can initialize any selected volume and use it in subsequent processes to store confidential data.

The process of encrypting a data disk involves treating a physical data disk (or a physical partition on the data disk) as a volume and encrypting it using CryptPilot.

The main operations on a volume are:
- **Initialization** (init): Initialize the volume so that it can be used to store encrypted data. This will erase the original data on the volume and create an encrypted volume with blank content.
- **Open**: Decrypt the already initialized volume using the configured credentials and create a virtual block device bearing plaintext at `/dev/mapper/${volume-name}`. Any content written on this block device will be encrypted and stored on the actual physical block device.
- **Close**: Close a specified volume

### Volume Configuration

When defining a volume using CryptPilot, you first need to place a corresponding configuration file under the `${config_dir}/volumes/` directory. For example, `${config_dir}/volumes/example.toml`

Here is a configuration file example for a volume encrypted with a one-time password: [otp.toml.template](/dist/etc/volumes/otp.toml.template)

> [!NOTE]
> The configuration file name must end with `.toml`, and its content should be in TOML format. Files not ending with `.toml` will be ignored. It is recommended to keep the configuration file name consistent with the volume name, but this is not mandatory.

Each volume contains the following configuration items:

```toml
# The name of the resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
volume = "data0"
# The path to the underlying encrypted device.
dev = "/dev/nvme1n1p1"
# Whether or not to open the LUKS2 device and set up mapping during booting. The default value is false.
auto_open = true
# The file system to initialize on the volume. Allowed values are ["swap", "ext4", "xfs", "vfat"]. If not specified, or if the device is not "empty", i.e., it contains any signature, the operation will be skipped.
makefs = "ext4"
# Whether or not to enable support for data integrity. The default value is false. Note that integrity cannot prevent a replay (rollback) attack.
integrity = true

# One Time Password (Temporary volume)
[encrypt.otp]
```

- `name`: The name of the volume, used to identify the volume.
- `dev`: The path to the underlying block device corresponding to the volume.
- `auto_open`: (Optional, default is `false`) Indicates whether to automatically open this volume during the boot process. This option can be used in conjunction with `/etc/fstab` to achieve automatic mounting of filesystems on encrypted volumes.
- `makefs`: (Optional) Indicates whether to automatically create a volume file system during initialization. Supported options are `"swap"`, `"ext4"`, `"xfs"`, `"vfat"`.
- `integrity`: (Optional, default is `false`) Indicates whether to enable data integrity protection. When enabled, data will be verified every time it is read to protect data integrity.
- `encrypt`: Indicates the credential storage type used for encryption of this volume. Please refer to the [Credential Storage Types](#credential-storage-types) section.


## System Disk Encryption

System disk encryption, also known as Full Disk Encryption (FDE), means encrypting the entire system disk. This scheme provides protection against root partition access via encryption and integrity protection mechanisms, and CryptPilot can also measure the root filesystem.

A system disk encrypted using CryptPilot is a GPT-partitioned disk containing two main volumes. These are a read-only rootfs volume and a writable data volume. The rootfs and data volumes can be configured with different passwords respectively.

You can refer to the steps in [README.md](README.md) to encrypt your system disk using CryptPilot.

### Configuration File Description

Here is a reference template for a configuration file [fde.toml.template](/dist/etc/fde.toml.template).

A basic system disk encryption configuration file should contain at least the `[rootfs]` and `[data]` configuration items, corresponding to the configuration of the rootfs and data volumes respectively.

#### Rootfs Volume

The rootfs volume stores the read-only root partition filesystem. Encrypting this filesystem is optional. However, regardless of whether encryption is enabled, this volume will be measured during startup and protected from modification based on dm-verity. During the boot phase, an overlayfs-based overlay layer will be placed over the read-only root filesystem, allowing temporary write modifications on the root partition without damaging the read-only layer or affecting the measurement of the read-only root partition.

The rootfs volume includes the following configuration items:

- `rw_overlay`: (Optional, default is `disk`) The storage location of the overlay layer above the root filesystem, can be `disk` or `ram`. When set to `disk`, the overlay layer will be stored on the disk's data volume (see [Data Volume](#data-volume) below). When set to `ram`, the overlay layer will be retained in memory and cleared upon instance restart or shutdown.

- `encrypt`: (Optional, default is unencrypted) The credential storage type for the rootfs volume. This field is optional; if not specified, the root partition will not be encrypted by default. Please refer to [Credential Storage Types](#credential-storage-types) for configuring this field, all credential storage types except `otp` are supported here.

##### Measurement

###### Measurement Principle

CryptPilot uses Remote Attestation technology to measure the root filesystem. This feature depends on the Attestation-Agent service running in the system. By recording the measurement value of the root filesystem in a non-rollback EventLog combined with the kernel's dm-verity mechanism, integrity protection of the root filesystem is achieved.

After entering the system, you can check the EventLog recorded by CryptPilot via `/run/attestation-agent/eventlog`:

```txt
# cat /run/attestation-agent/eventlog
INIT sha384/000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
cryptpilot.alibabacloud.com load_config {"alg":"sha384","value":"b8635580d85cb0a2b5896664eb795cadb99a589783817c81e263f6752f2a735d2705b4638947de3d947231b76b5a1877"}
cryptpilot.alibabacloud.com fde_rootfs_hash a3f73f5b995e7d8915c998d9f1e56b0e063a6e20c2bbb512e88e8fbc4e8f2965
cryptpilot.alibabacloud.com initrd_switch_root {}
```

As shown above, three EventLogs will be recorded during the CryptPilot startup process:

| Domain | Operation | Example Value | Description |
| --- | --- | --- | --- |
| cryptpilot.alibabacloud.com | load_config | `{"alg":"sha384","value":"b8635580d85cb0a2b5896664eb795cadb99a589783817c81e263f6752f2a735d2705b4638947de3d947231b76b5a1877"}` | The hash value of the configuration file used by CryptPilot |
| cryptpilot.alibabacloud.com | fde_rootfs_hash | `a3f73f5b995e7d8915c998d9f1e56b0e063a6e20c2bbb512e88e8fbc4e8f2965` | The measurement value of the decrypted rootfs volume |
| cryptpilot.alibabacloud.com | initrd_switch_root | `{}` | An event record indicating that the system has switched from the initrd phase to the real system. The value of this item is always `{}` |

After entering the system, the business can locally verify the system startup process based on the EventLog generated by this measurement mechanism or provide it to a trusted entity for verification via remote attestation.

###### Using `kbs` as the Credential Storage Type

During the startup process, if `kbs` is used as the storage type for the rootfs or data volume, the measurement information will be automatically carried when accessing the KBS service to obtain the decryption credentials for the volume. The owner of the KBS service can configure the corresponding [Remote Attestation Policy](https://github.com/openanolis/trustee/blob/b1a278a4360b9b47f82001b5c3d350b8c154acf5/attestation-service/docs/policy.md) for validation, thereby achieving full chain trust for CVM startup.


#### Data Volume

The data volume is an encrypted volume composed of the remaining available space on the system disk, containing a writable Ext4 filesystem. During the system startup process, this volume will be decrypted, and upon entering the system, this volume will be mounted at the `/data` location. Any data written to the data volume will be encrypted before being written to disk. Users can write their data files here, and the data will not be lost after the instance restarts.

The data volume includes the following configuration items:

- `integrity`: (Optional, default is `false`), indicates whether to enable data integrity protection. When enabled, data will be verified every time it is read from the disk, preventing data tampering.
- `encrypt`: The credential storage type for the data volume. Please refer to [Credential Storage Types](#credential-storage-types) for configuring this field. All credential storage types except `otp` are supported here.


## Credential Storage Types

Through modular design, CryptPilot supports obtaining the decryption keys for volumes from various credential storage types. The document records the implemented credential storage types. As versions iterate, the supported storage types will increase.

### `[encrypt.otp]`: One-Time Password OTP

This is a special credential storage type indicating that CryptPilot uses a secure random number-generated one-time password to encrypt the volume. This password will be one-time, meaning that volumes using this credential storage type do not require an initialization process, and every open will automatically trigger a data wipe operation. Thus, each open results in a new volume, suitable for scenarios requiring temporary encrypted data storage.

Configuration file example: [otp.toml.template](/dist/etc/volumes/otp.toml.template)

### `[encrypt.kbs]`: Key Broker Service (KBS)

Indicates that credentials are hosted in [Key Broker Service (KBS)](https://github.com/openanolis/trustee/tree/main/kbs#key-broker-service) and authenticated using Remote Attestation to obtain credentials for decrypting volumes.

Configuration file example: [kbs.toml.template](/dist/etc/volumes/kbs.toml.template)

### `[encrypt.kms]`: Key Management Service KMS (Access Key)

Indicates that credentials are hosted in [Alibaba Cloud Key Management Service KMS](https://yundun.console.aliyun.com/) and authenticated using the given Access Key to obtain credentials.

Configuration file example: [kms.toml.template](/dist/etc/volumes/kms.toml.template)

### `[encrypt.oidc]`: Key Management Service KMS (OIDC)

Indicates that credentials are hosted in [Alibaba Cloud Key Management Service KMS](https://yundun.console.aliyun.com/). And requires authentication through the OIDC (OpenID Connect) protocol to obtain credentials.

This credential storage type allows configuring an external program providing the OIDC token. CryptPilot will execute this external program to obtain the OIDC token and use it in subsequent processes to obtain credentials from KMS.

Configuration file example: [oidc.toml.template](/dist/etc/volumes/oidc.toml.template)

### `[encrypt.exec]`: Executable Program Providing Credentials (EXEC)

This is a special credential storage type indicating that CryptPilot obtains credentials for decrypting volumes by executing an external program and acquiring credentials from the standard output (stdout) of this external program.

> [!NOTE]
> The standard output data of this external program will be directly regarded as the decryption credentials without trimming, string conversion, or involving base64 decoding. Therefore, you need to ensure there are no extra invisible characters such as carriage returns and spaces.

Configuration file example: [exec.toml.template](/dist/etc/volumes/exec.toml.template)
