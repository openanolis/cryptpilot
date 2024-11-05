对于toml语法，请查看：https://toml.io/en/

调试生成的initramfs.img：lsinitrd


yum install rpm-build

rpmbuild -ba cryptpilot.spec
yum-builddep cryptpilot.spec

rpm -qlp /root/rpmbuild/RPMS/x86_64/cryptpilot-0.1.0-1.al8.x86_64.rpm
rpm -i /root/rpmbuild/RPMS/x86_64/cryptpilot-0.1.0-1.al8.x86_64.rpm
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

## Test

```sh
make example-prepare
make example-run
make example-run-test
```
