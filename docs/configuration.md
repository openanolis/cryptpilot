# Configuration (TBD)

cryptpilot uses some configuration file to configure the encryption and other settings. The configuration file is in the TOML format.

> For the syntax of TOML, please refer to: https://toml.io/en/




调试生成的initramfs.img：lsinitrd


yum install rpm-build

rpmbuild -ba cryptpilot.spec
yum-builddep cryptpilot.spec

rpm -qlp /root/rpmbuild/RPMS/x86_64/cryptpilot-*.al8.x86_64.rpm
rpm -i /root/rpmbuild/RPMS/x86_64/cryptpilot-*.al8.x86_64.rpm
rpmquery -l nginx

LC_ALL=C rpmbuild -ba cryptpilot.spec

/usr/lib/systemd/systemd-cryptsetup attach data0 /dev/nvme1n1p1 /var/run/cryptpilot.sock

systemd-analyze plot > /tmp/seq.svg
systemd-analyze dot --to-pattern='*.target' --from-pattern='*.target' --to-pattern='*.service' --from-pattern='*.service' \
      | dot -Tsvg >/tmp/targets.svg

musl-gcc或者：
https://toolchains.bootlin.com/downloads/releases/toolchains/x86-64/tarballs/x86-64--musl--stable-2024.05-1.tar.xz

findmnt

partprobe刷新内核中的磁盘信息


dracut -v -f

```shell
cat <<EOF >>/etc/grub.d/40_custom
menuentry "Alibaba Cloud Linux (5.10.134-17.3.al8.x86_64) (cryptpilot test another disk)" {
    insmod part_gpt
    insmod ext2
    set root='(hd2,gpt3)'
    linux /vmlinuz-5.10.134-17.3.al8.x86_64
    initrd /initramfs-5.10.134-17.3.al8.x86_64.img
}
EOF

cat <<EOF >>/etc/grub.d/40_custom
menuentry "Alibaba Cloud Linux (5.10.134-17.3.al8.x86_64) (cryptpilot test w/o specify root)" {
    insmod part_gpt
    insmod ext2
    set root='(hd0,gpt3)'
    linux /boot/vmlinuz-5.10.134-16.1.al8.x86_64
    initrd /boot/initramfs-5.10.134-16.1.al8.x86_64.img
}
EOF

grub2-mkconfig -o /boot/efi/EFI/alinux/grub.cfg
```

- 阿里云ECS 自定义数据
  - https://help.aliyun.com/zh/ecs/user-guide/view-instance-metadata
  - https://help.aliyun.com/zh/ecs/user-guide/customize-the-initialization-configuration-for-an-instance
  - https://help.aliyun.com/zh/auto-scaling/use-cases/enable-the-instance-user-data-feature-to-automatically-configure-ecs-instances#section-kfp-0d6-w4t
  - https://cloudinit.readthedocs.io/en/latest/reference/datasources/aliyun.html
