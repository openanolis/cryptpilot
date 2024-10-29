
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
            mkpart data0 0% 50% \
            mkpart swap0 50% 100%

sudo mkfs.ext4 /dev/nvme1n1p1
sudo mkswap /dev/nvme1n1p2
# Now we have all the partition allocated and filesystem initialized (UUID is ready now).

# Force the UUID values for easy testing
sudo tune2fs -U cf60541e-9ffd-4dc6-b0b8-8d20fbf51c68 /dev/nvme1n1p1
sudo mkswap -U 6f0e01bd-8155-424d-a2c5-befd83325070 /dev/nvme1n1p2

# Append to /etc/fstab
new_line="UUID=cf60541e-9ffd-4dc6-b0b8-8d20fbf51c68 /mnt/data0 ext4 defaults,nofail 0 2"
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

2. 测试

```sh
# 提前将目标盘初始化为LUKS2卷，不然systemd-cryptsetup attach会报 "Failed to load LUKS superblock on device Invalid argument"
echo -n "your_passphrase" | cryptsetup luksFormat --type luks2 /dev/nvme1n1p1 -

# 运行key-supplier，监听在/var/run/cryptpilot.sock
cargo run -- -d ./examples crypttab-key-supplier

# 触发attach，这个和写在/etc/crypttab里面是一个远离
/usr/lib/systemd/systemd-cryptsetup attach data0 /dev/nvme1n1p1 /var/run/cryptpilot.sock
```

注意上面的例子在key_provider为temp时实际上并不能运行。表现为如下的密码错误：
```txt
[root@iZ2ze4z8h15v23azdr665lZ ~]# /usr/lib/systemd/systemd-cryptsetup attach data0 /dev/nvme1n1p1 /var/run/cryptpilot.sock
Set cipher aes, mode xts-plain64, key size 512 bits for device /dev/nvme1n1p1.
Failed to activate with key file '/var/run/cryptpilot.sock'. (Key data incorrect?)
Please enter passphrase for disk data0! ********************************
Set cipher aes, mode xts-plain64, key size 512 bits for device /dev/nvme1n1p1.
Failed to activate with specified passphrase. (Passphrase incorrect?)
Please enter passphrase for disk data0! ********************************
Set cipher aes, mode xts-plain64, key size 512 bits for device /dev/nvme1n1p1.
Failed to activate with specified passphrase. (Passphrase incorrect?)
Too many attempts; giving up.
```
但密码实际上已经初始化到该LUKS2卷上，因此猜测其实systemd-cryptsetup的实现本来就不允许在调/var/run/cryptpilot.sock拿密码的时候还对LUKS2卷做重新初始化操作。


