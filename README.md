对于toml语法，请查看：https://toml.io/en/

调试生成的initramfs.img：lsinitrd


yum install rpm-build

rpmbuild -ba cryptpilot.spec
yum-builddep cryptpilot.spec


/usr/lib/systemd/systemd-cryptsetup attach data0 /dev/nvme1n1p1 /var/run/cryptpilot.sock


## Test

1. Init the data disk

```sh
sudo parted --script /dev/nvme1n1 \
            mktable gpt \
            mkpart data0 0% 33% \
            mkpart data1 33% 66% \
            mkpart swap0 66% 100%

sudo mkfs.ext4 /dev/nvme1n1p1
sudo mkfs.ext4 /dev/nvme1n1p2
sudo mkswap /dev/nvme1n1p3
# Now we have all the partition allocated and filesystem initialized (UUID is ready now).

# Force the UUID values for easy testing
sudo tune2fs -U cf60541e-9ffd-4dc6-b0b8-8d20fbf51c68 /dev/nvme1n1p1
sudo tune2fs -U 07f5f317-242c-4707-9d90-c8db72ae2a64 /dev/nvme1n1p1
sudo mkswap -U 6f0e01bd-8155-424d-a2c5-befd83325070 /dev/nvme1n1p3

# Append to /etc/fstab
new_line="UUID=cf60541e-9ffd-4dc6-b0b8-8d20fbf51c68 /mnt/data0 ext4 defaults,nofail 0 2"
if ! grep -Fxq "$new_line" /etc/fstab; then
    echo "$new_line" | sudo tee -a /etc/fstab > /dev/null
fi
new_line="UUID=07f5f317-242c-4707-9d90-c8db72ae2a64 /mnt/data1 ext4 defaults,nofail 0 2"
if ! grep -Fxq "$new_line" /etc/fstab; then
    echo "$new_line" | sudo tee -a /etc/fstab > /dev/null
fi
new_line="UUID=6f0e01bd-8155-424d-a2c5-befd83325070 none swap defaults,nofail 0 0"
if ! grep -Fxq "$new_line" /etc/fstab; then
    echo "$new_line" | sudo tee -a /etc/fstab > /dev/null
fi
```

重启后应该有如下效果：

```txt
[root@iZ2ze4z8h15v23azdr665lZ ~]# lsblk
NAME        MAJ:MIN RM   SIZE RO TYPE MOUNTPOINT
nvme0n1     259:0    0   500G  0 disk
├─nvme0n1p1 259:1    0   200M  0 part /boot/efi
├─nvme0n1p2 259:2    0     2M  0 part
└─nvme0n1p3 259:3    0 499.8G  0 part /
nvme1n1     259:4    0    20G  0 disk
├─nvme1n1p1 259:5    0    10G  0 part /mnt/data0
└─nvme1n1p2 259:6    0    10G  0 part [SWAP]
```

取消挂载
```sh
umount /mnt/data0
swapoff -U 6f0e01bd-8155-424d-a2c5-befd83325070
```

