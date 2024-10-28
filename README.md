# cryptpilot: The confidentiality for OS booting and data at rest in confidential computing environments

The cryptpilit project aims to provide a way that allows you to securely boot your system while ensuring the encryption and measurability of the entire operating system, as well as encryption and integrity protection for data at rest.

## Build and Install

It is recommended to build a RPM package and install it on your system.

```sh
make rpm-build
```

Then install the rpm package on your system:

```sh
rpm --install /root/rpmbuild/RPMS/x86_64/cryptpilot-0.1.0-1.al8.x86_64.rpm
```

Now you can edit the configuration files under `/etc/cryptpilot/`. See the [configuration](docs/configuration.md) for details.

Don't forget to update the initramfs after changing the configuration files.

```sh
dracut -vvvv -f
```

## Example: encrypt a bootable OS

In this example, we will show how to encrypt a bootable OS. The OS can be on a OS disk image file or a real system disk. 

### Convert a OS disk image file

We will use the Alinux3 disk image file from [here](https://mirrors.aliyun.com/alinux/3/image/).

1. Download the Alinux3 disk image file (KVM x86_64 version with Microsoft Virtual PC format):

```sh
wget https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20240819.vhd
```

2. Convert the disk image file:

Here we will encrypt the disk image file with a provided passphrase (GkdQgrmLx8LkGi2zVnGxdeT) and configs from `./examples/` directory. The second parameter is the output file name.

```sh
./cryptpilot-convert.sh ./aliyun_3_x64_20G_nocloud_alibase_20240819.vhd ./aliyun_3_x64_20G_nocloud_alibase_20240819_cc.vhd ./examples/ GkdQgrmLx8LkGi2zVnGxdeT
```

3. Upload the converted disk image file to Aliyun and boot from it.

### Convert a real system disk

For those who wish to convert a real system disk, you need to unbind the disk from the original instance and bind it to another instance (DO NOT convert the active disk where you are booting from).

1. Convert the disk (assuming the disk is `/dev/nvme2n1`):

```sh
./cryptpilot-convert.sh /dev/nvme2n1 ./examples/ GkdQgrmLx8LkGi2zVnGxdeT

```

Now re-bind the disk to the original instance and boot from it.

## Example: setting up encrypted data partations

In this example, we will create some encrypted volumes and each of them with different configurations. For the configuration files for each volume, please refer to the [examples/volumes](examples/volumes) directory.

To run this example, you need to bind another disk to your system (`/dev/nvme1n1`). It can be a disk in any size but at least 20G.

1. Prepare the configs and create partition tables:

```sh
make example-prepare
```

2. Run the example, and check that we have created some encrypted volumes and opened them.

```sh
make example-run
```

```sh
# Check
cryptpilot show
```

3. To test the reliability of encrypted volumes, run some filesystem test suites on the encrypted volumes.

```sh
make example-run-test
```
