
调试生成的initramfs.img：lsinitrd


yum install rpm-build

rpmbuild -ba cryptpilot.spec
yum-builddep cryptpilot.spec
