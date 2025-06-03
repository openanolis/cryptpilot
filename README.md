# cryptpilot: The confidentiality for OS booting and data at rest in confidential computing environments
[![Building](/../../actions/workflows/build-rpm.yml/badge.svg)](/../../actions/workflows/build-rpm.yml)
![GitHub Release](https://img.shields.io/github/v/release/openanolis/cryptpilot)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

The cryptpilot project aims to provide a way that allows you to securely boot your system while ensuring the encryption and measurability of the entire operating system, as well as encryption and integrity protection for data at rest.


# Installation

You can found and install the prebuilt binaries the [latest release](https://github.com/openanolis/cryptpilot/releases). Or if you want to build it yourself, you can follow instructions from the [development guide](docs/development.md).

After installing, you can edit the configuration files under `/etc/cryptpilot/`. See the [configuration](docs/configuration.md) for details.


## Example: encrypt a bootable OS

In this example, we will show how to encrypt a bootable OS. The OS can be on a OS disk image file or a real system disk. 

Remenber that you have to prepare the configs directory before you start. The configs directory is a normal cryptpilot config dir (the struct is just like the global config dir `/etc/cryptpilot/`), and should contains at least one `fde.toml` config file. The full details of the configuration can be found in [docs/configuration.md](docs/configuration.md) and you may would like to read it first.

Here we will create a `config_dir` with a single `fde.toml` config file. And for demo purposes, we will use `exec` key provider, which returns a hardcoded passphrase `AAAaaawewe222` for both `rootfs` and `data` volume:

> [!IMPORTANT]
> The `exec` key provider below is only for demo purposes, and you should choose another key provider (e.g. `kbs` or `kms`) in production.

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

tree
```

Here is the content of the `config_dir`:
```txt
./config_dir
└─── fde.toml
```

You can check your configs are valid with:

```sh
cryptpilot -d ~/diskenc/config_dir_exec/ config check --keep-checking
```

### Encrypt a OS disk image file

We will use the Alinux3 disk image file from [here](https://mirrors.aliyun.com/alinux/3/image/).

1. Download the Alinux3 disk image file (KVM x86_64 version with Microsoft Virtual PC format):

```sh
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20250117.qcow2
```

2. Encrypt the disk image file:

Here we will encrypt the disk image file with a provided passphrase (`AAAaaawewe222`) and configs from `./config_dir/` directory. The encrypted disk file is specified by `--out` parameter.

And you can start the encryption with:

```sh
cryptpilot-convert --in ./aliyun_3_x64_20G_nocloud_alibase_20250117.qcow2 --out ./encrypted.qcow2 -c ./config_dir/ --passphrase AAAaaawewe222
```

> Note: You can also use the --package parameter to install some packages/rpms to the disk, before the encryption.


3. (optional) Test the converted disk image file:

You can launch a virtual machine with the converted disk image file and check that it works.

> Note: If you are using a `.vhd` file, you have to convert it to a `.qcow2` file first, before you launch it with qemu.

```sh
yum install -y qemu-kvm
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/seed.img

/usr/libexec/qemu-kvm \
    -m 4096M \
    -smp 4 \
    -nographic \
    -drive file=./encrypted.qcow2,format=qcow2,if=virtio,id=hd0,readonly=off \
    -drive file=./seed.img,if=virtio,format=raw
```

> Note: Accroding to [this page](https://www.alibabacloud.com/help/zh/alinux/getting-started/use-alibaba-cloud-linux-3-images-in-an-on-premises-environment), the login username of this is `alinux`, and password is `aliyun`.

After you finished your tests, you can use Ctrl-A C to get to the qemu console, and enter 'quit' to exit qemu.

4. Upload the encrypted disk image file to Aliyun and boot from it.

### Encrypt a real system disk

For those who wish to encrypt a real system disk, you need to unbind the disk from the original instance and bind it to another instance (DO NOT encrypt the active disk where you are booting from).

1. Encrypt the disk (assuming the disk is `/dev/nvme2n1`):

```sh
cryptpilot-convert --device /dev/nvme2n1 -c ./config_dir/ --passphrase AAAaaawewe222
```

Now re-bind the disk to the original instance and boot from it.

## Example: setting up encrypted data partations

In this example, we will create some encrypted volumes and each of them with different configurations. For the configuration files for each volume, please refer to the [dist/etc/](dist/etc) directory.

To run this example, you need to bind another empty disk to your system (`/dev/nvme1n1`). It can be a disk in any size.

1. Create partition tables on the disk:

In this example we uses GPT partition table, with one primary partition.
```sh
parted --script /dev/nvme1n1 \
            mktable gpt \
            mkpart part1 0% 100%
```

2. Create a config for `data0` volume

```sh
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.otp]
```

This volume will be encrypted with One-Time-Password, which means the data on it is volatile, and will be lost after closing. The volume will be automatically opened during system startup.

2. Open the volume, and check that we have created a encrypted volume.

```sh
cryptpilot open data0
```

```sh
cryptpilot show
```

It may outputs like this:

```txt
╭────────┬───────────────────┬─────────────────┬──────────────┬──────────────────┬──────────────┬────────╮
│ Volume ┆ Volume Path       ┆ Underlay Device ┆ Key Provider ┆ Extra Options    ┆ Initialized  ┆ Opened │
╞════════╪═══════════════════╪═════════════════╪══════════════╪══════════════════╪══════════════╪════════╡
│ data0  ┆ /dev/mapper/data0 ┆ /dev/nvme1n1p1  ┆ otp          ┆ auto_open = true ┆ Not Required ┆ True   │
│        ┆                   ┆                 ┆              ┆ makefs = "ext4"  ┆              ┆        │
│        ┆                   ┆                 ┆              ┆ integrity = true ┆              ┆        │
│        ┆                   ┆                 ┆              ┆                  ┆              ┆        │
╰────────┴───────────────────┴─────────────────┴──────────────┴──────────────────┴──────────────┴────────╯
```

Here the volume is opened and it's path is `/dev/mapper/data0`, which contains the plaintext.


3. The volume is formated with an ext4 file system. You have to mount it before using it.

```sh
mkdir -p /mnt/data0
mount /dev/mapper/data0 /mnt/data0
```

Now you can read and write files in the mounted directory.

4. If you want to setup auto open for the volume, you should first set `auto_open = true` in the volume config file. And then you can use the following command to enable the service:

```sh
systemctl enable --now cryptpilot.service
```

# Supported Distrubutions

CryptPilot has been tested on the following distributions, and it may not work on other distributions.

- [Anolis OS 8](https://openanolis.cn/anolisos/8)
- [Anolis OS 23](https://openanolis.cn/anolisos/23)
- [Alibaba Cloud Linux 3](https://www.aliyun.com/product/alinux)


# License

Apache-2.0