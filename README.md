# cryptpilot: The confidentiality for OS booting and data at rest in confidential computing environments
[![Building](/../../actions/workflows/build-rpm.yml/badge.svg)](/../../actions/workflows/build-rpm.yml)
![GitHub Release](https://img.shields.io/github/v/release/openanolis/cryptpilot)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

The cryptpilit project aims to provide a way that allows you to securely boot your system while ensuring the encryption and measurability of the entire operating system, as well as encryption and integrity protection for data at rest.


# Installation

You can found and install the prebuilt binaries the [latest release](https://github.com/openanolis/cryptpilot/releases). Or if you want to build it yourself, you can follow instructions from the [development guide](docs/development.md).

After installing, you can edit the configuration files under `/etc/cryptpilot/`. See the [configuration](docs/configuration.md) for details.


## Example: encrypt a bootable OS

In this example, we will show how to encrypt a bootable OS. The OS can be on a OS disk image file or a real system disk. 

### Convert a OS disk image file

We will use the Alinux3 disk image file from [here](https://mirrors.aliyun.com/alinux/3/image/).

1. Download the Alinux3 disk image file (KVM x86_64 version with Microsoft Virtual PC format):

```sh
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20250117.vhd
```

2. Convert the disk image file:

Here we will encrypt the disk image file with a provided passphrase (GkdQgrmLx8LkGi2zVnGxdeT) and configs from `./config_dir/` directory. The second parameter is the output file name.

```sh
./cryptpilot-convert.sh ./aliyun_3_x64_20G_nocloud_alibase_20250117.vhd ./aliyun_3_x64_20G_nocloud_alibase_20250117_cc.vhd ./config_dir/ GkdQgrmLx8LkGi2zVnGxdeT
```

3. Upload the converted disk image file to Aliyun and boot from it.

### Convert a real system disk

For those who wish to convert a real system disk, you need to unbind the disk from the original instance and bind it to another instance (DO NOT convert the active disk where you are booting from).

1. Convert the disk (assuming the disk is `/dev/nvme2n1`):

```sh
./cryptpilot-convert.sh /dev/nvme2n1 ./config_dir/ GkdQgrmLx8LkGi2zVnGxdeT

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
