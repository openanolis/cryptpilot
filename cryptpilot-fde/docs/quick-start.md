# Quick Start Guide: cryptpilot-fde

This guide walks you through encrypting a bootable OS disk with full disk encryption.

## Prerequisites

- cryptpilot-fde installed on your system
- A bootable qcow2 disk image, or an unmounted real disk

## Prepare Configuration

Before encrypting, you need to prepare a configuration directory with at least one `fde.toml` file. The configuration directory structure is similar to `/etc/cryptpilot/`.

For this demo, we'll use the `exec` key provider with a hardcoded passphrase:

> [!IMPORTANT]
> The `exec` key provider below is only for demo purposes. Use `kbs` or `kms` in production.

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF

tree ./config_dir
```

The configuration directory structure:

```txt
./config_dir
└── fde.toml
```

**Configuration Explanation:**

- `[rootfs]`: Root filesystem configuration
  - `rw_overlay = "disk"`: Store writable overlay on data partition (survives reboot)
  - `encrypt.exec`: Use exec provider with passphrase "AAAaaawewe222"
- `[data]`: Data partition configuration
  - `integrity = true`: Enable dm-integrity for data authenticity
  - `encrypt.exec`: Use exec provider with passphrase "AAAaaawewe222"

### Validate Configuration

Check if your configuration is valid:

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

## Example 1: Encrypt a Disk Image File

This example shows how to encrypt an existing bootable disk image.

### Step 1: Download Disk Image

We'll use the Alibaba Cloud Linux 3 disk image:

```sh
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2
```

### Step 2: Encrypt the Disk Image

Encrypt the disk image with the prepared configuration:

```sh
cryptpilot-convert --in ./aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2 \
    --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222
```

**What happens during encryption:**

1. Reads the original disk image
2. Creates encrypted rootfs partition with dm-verity
3. Creates encrypted data partition with dm-integrity
4. Installs cryptpilot-fde into initrd
5. Configures boot loader for encrypted boot
6. Writes the encrypted disk to output file

**Optional:** You can install additional packages during encryption:

```sh
cryptpilot-convert --in ./source.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222 \
    --package /path/to/package.rpm
```

### Step 3: Test the Encrypted Disk (Optional)

Launch a VM to test the encrypted disk:

```sh
# Install qemu-kvm
yum install -y qemu-kvm

# Download seed image for cloud-init
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/seed.img

# Launch VM
/usr/libexec/qemu-kvm \
    -m 4096M \
    -smp 4 \
    -nographic \
    -drive file=./encrypted.qcow2,format=qcow2,if=virtio,id=hd0,readonly=off \
    -drive file=./seed.img,if=virtio,format=raw
```

> **Login credentials:** Username: `alinux`, Password: `aliyun`

**Exit QEMU:** Press `Ctrl-A` then `C` to enter QEMU console, then type `quit`.

### Step 4: Calculate Reference Values

For attestation purposes, calculate cryptographic reference values:

```sh
cryptpilot-fde show-reference-value --stage system --disk ./encrypted.qcow2
```

This outputs measurement values that can be uploaded to [Reference Value Provider Service (RVPS)](https://github.com/confidential-containers/trustee/tree/main/rvps).

### Step 5: Upload and Boot

Upload the encrypted disk image to your cloud provider (e.g., Alibaba Cloud) and boot from it.

## Example 2: Measure-Only rootfs (No Encryption)

For some scenarios, you may only need integrity protection and measurement for rootfs without encryption. In this case, rootfs uses dm-verity protection but is not encrypted.

> [!NOTE]
> This mode is suitable for scenarios where:
> - rootfs contains no sensitive data
> - Only integrity validation and measurement are needed, not confidentiality
> - Reduced performance overhead during boot is desired

### Configuration

Create configuration without rootfs encryption:

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"
# Note: No encrypt configuration in rootfs section

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF
```

**Configuration Explanation:**

- `[rootfs]`: Root filesystem configuration
  - `rw_overlay = "disk"`: Store writable overlay on data partition
  - **No `encrypt` configuration**: rootfs is not encrypted, only dm-verity integrity protection
- `[data]`: Data partition configuration
  - `integrity = true`: Enable dm-integrity
  - `encrypt.exec`: Data partition is still encrypted

### Encrypt Data Partition (rootfs Not Encrypted)

Use the `--rootfs-no-encryption` parameter:

```sh
cryptpilot-convert --in ./aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2 \
    --out ./encrypted.qcow2 \
    -c ./config_dir/ \
    --rootfs-no-encryption
```

**What happens:**

1. rootfs uses dm-verity for integrity protection (not encrypted)
2. Data partition is encrypted normally
3. System still performs measurement and attestation during boot
4. rootfs is mounted read-only with writable overlay layer

### Use Cases

This configuration is suitable for:

- ✅ rootfs contains only public system files
- ✅ Need to verify system integrity (tamper detection)
- ✅ Need remote attestation to confirm system is unmodified
- ✅ Want to reduce decryption performance overhead for rootfs
- ❌ Not suitable when rootfs contains sensitive configurations or keys

## Example 3: Encrypt a Real System Disk

For production systems, you need to encrypt a real disk.

> [!IMPORTANT]
> **DO NOT encrypt the active disk you are booting from!**
> 
> You must:
> 1. Unbind the disk from the instance
> 2. Bind it to another instance as a data disk
> 3. Encrypt it
> 4. Re-bind it to the original instance

### Steps

1. **Prepare configuration** (same as above):

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]
EOF
```

2. **Validate configuration**:

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

3. **Encrypt the disk** (assuming the disk is `/dev/nvme2n1`):

```sh
cryptpilot-convert --device /dev/nvme2n1 \
    -c ./config_dir/ \
    --rootfs-passphrase AAAaaawewe222
```

4. **Re-bind the disk** to the original instance and boot from it.

## Example 4: Using KBS Provider (Production)

For production environments, use Key Broker Service with remote attestation.

### Configuration

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/rootfs-key"

[data]
integrity = true

[data.encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data-key"
EOF
```

### Encrypt

```sh
# For disk images
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase <actual-rootfs-key>

# For real disks
cryptpilot-convert --device /dev/nvme2n1 \
    -c ./config_dir/ --rootfs-passphrase <actual-rootfs-key>
```

### Boot Process

When booting, the system will:

1. Generate attestation evidence in TEE
2. Send evidence to KBS
3. KBS verifies the evidence
4. If verified, KBS returns the decryption key
5. System decrypts and boots

## Example 5: Using KMS Provider (Cloud-Managed)

For Alibaba Cloud users, use KMS for centralized key management.

### Configuration

```sh
mkdir -p ./config_dir
cat << EOF > ./config_dir/fde.toml
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"

[data]
integrity = true

[data.encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
EOF
```

### Encrypt

```sh
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase <from-kms>
```

## Running in Docker

If you're not on a [supported distribution](#supported-distributions), use Docker:

### Step 1: Load NBD Kernel Module

```sh
modprobe nbd max_part=8
```

### Step 2: Create Container

```sh
docker run -it --privileged --ipc=host \
    -v /run/udev/control:/run/udev/control \
    -v /dev:/dev \
    alibaba-cloud-linux-3-registry.cn-hangzhou.cr.aliyuncs.com/alinux3/alinux3:latest bash
```

> **Note:** The additional parameters (`--privileged --ipc=host -v /run/udev/control:/run/udev/control -v /dev:/dev`) are required to make `/dev` work properly in the container.

### Step 3: Install cryptpilot-fde

Inside the container, download and install cryptpilot-fde from the [Release page](https://github.com/openanolis/cryptpilot/releases):

```sh
# Download the latest RPM package
wget https://github.com/openanolis/cryptpilot/releases/download/vX.Y.Z/cryptpilot-fde-X.Y.Z-1.x86_64.rpm

# Install
rpm -ivh cryptpilot-fde-X.Y.Z-1.x86_64.rpm
```

> **Tip:** Replace `X.Y.Z` with the actual version number.

### Step 4: Run cryptpilot Commands

```sh
cryptpilot-fde --help
cryptpilot-convert --help
```

Now you can run any cryptpilot-fde commands inside the container.

## Troubleshooting

### Configuration Check Failed

If `config check` reports errors:

```sh
cryptpilot-fde -c ./config_dir/ config check --keep-checking
```

Common issues:
- Missing required fields in configuration
- Invalid key provider settings
- Incorrect file paths

### Conversion Failed

If `cryptpilot-convert` fails:

1. **Check disk format**: Only qcow2 images are supported for disk images
2. **Check disk size**: Ensure enough space for encryption overhead
3. **For real disks**: Ensure the disk is unmounted and not in use
4. **Device already exists error**: If you see errors like `/dev/system: already exists in filesystem`, it may be leftover from a previous failed convert. Try `dmsetup remove_all` to clean up
5. **Check logs**: The last convert's detailed log is saved at `/tmp/.cryptpilot-convert.log`

### Boot Failed

If the encrypted system fails to boot:

1. **Check key provider**: Ensure network/attestation is working
2. **Check reference values**: Verify measurements match expected values
3. **Check console output**: Look for error messages during boot

## Next Steps

- [Configuration Guide](configuration.md) - Detailed configuration options
- [Boot Process](boot.md) - How cryptpilot-fde integrates with boot
- [Key Providers](../../docs/key-providers.md) - Key provider configuration details
- [cryptpilot-enhance](cryptpilot_enhance.md) - Harden images before encryption

## See Also

- [cryptpilot-crypt Quick Start](../../cryptpilot-crypt/docs/quick-start.md) - Encrypt data volumes
- [Development Guide](../../docs/development.md) - Build and test instructions
