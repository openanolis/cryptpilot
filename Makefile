.PHONE: help depend build initrd sync distro commit

help:
	@echo "Read README.md first"

install-rpm-build-depend:
	[[ -e /opt/x86-64--musl--stable-2024.05-1 ]] || { curl -o /tmp/x86-64--musl--stable-2024.05-1.tar.xz -L -C - https://toolchains.bootlin.com/downloads/releases/toolchains/x86-64/tarballs/x86-64--musl--stable-2024.05-1.tar.xz || tar -xvf /tmp/x86-64--musl--stable-2024.05-1.tar.xz -C /opt/ && rm -f /tmp/x86-64--musl--stable-2024.05-1.tar.xz ; }

rpm-build: install-rpm-build-depend
	rpmbuild -ba cryptpilot.spec

rpm-install: rpm-build
	yum remove cryptpilot -y
	ls -t /root/rpmbuild/RPMS/x86_64/cryptpilot-*.rpm | head -n 1 | xargs rpm --install

example-prepare: example-clean
	parted --script /dev/nvme1n1 \
            mktable gpt \
            mkpart data0 0% 1024GiB \
            mkpart data1 1024GiB 1030GiB \
            mkpart data2 1030GiB 1036GiB \
            mkpart data3 1036GiB 1042GiB \
            mkpart swap0 1042GiB 100%
	partprobe
	rsync -avp ./examples/ /etc/cryptpilot/

example-clean:
	umount /mnt/data0 ; dmsetup remove data0 ; dmsetup remove data0_dif ; dd if=/dev/urandom of=/dev/nvme1n1p1 count=16 seek=0 bs=4096 ;
	umount /mnt/data1 ; dmsetup remove data1 ; dmsetup remove data1_dif ; dd if=/dev/urandom of=/dev/nvme1n1p2 count=16 seek=0 bs=4096 ;
	umount /mnt/data2 ; dmsetup remove data2 ; dmsetup remove data2_dif ; dd if=/dev/urandom of=/dev/nvme1n1p3 count=16 seek=0 bs=4096 ;
	umount /mnt/data3 ; dmsetup remove data3 ; dmsetup remove data3_dif ; dd if=/dev/urandom of=/dev/nvme1n1p4 count=16 seek=0 bs=4096 ;
	[ -e /dev/mapper/swap0 ] && swapoff /dev/mapper/swap0 ; dmsetup remove swap0 ; dmsetup remove swap0_dif ; dd if=/dev/urandom of=/dev/nvme1n1p5 count=16 seek=0 bs=4096 ;

example-run: example-clean
	cryptpilot init data0 -y && cryptpilot open data0 && mkdir -p /mnt/data0 && mount -t ext4 /dev/mapper/data0 /mnt/data0
	cryptpilot init data1 -y && cryptpilot open data1 && mkdir -p /mnt/data1 && mount -t ext4 /dev/mapper/data1 /mnt/data1 && echo -n test > /mnt/data1/testfile
	umount /mnt/data1 && cryptpilot open data1 && mkdir -p /mnt/data1 && mount -t ext4 /dev/mapper/data1 /mnt/data1 && [[ `cat /mnt/data1/testfile` == "test" ]]
	cryptpilot init data2 -y && cryptpilot open data2 && mkdir -p /mnt/data2 && mount -t ext4 /dev/mapper/data2 /mnt/data2
	cryptpilot init data3 -y && cryptpilot open data3 && mkdir -p /mnt/data3 && mount -t xfs /dev/mapper/data3 /mnt/data3
	cryptpilot init swap0 -y && cryptpilot open swap0 && swapon /dev/mapper/swap0
	$(info All is done. Now you can check with 'findmnt' and 'swapon')

example-run-test: install-test-depend
	{ [[ -e /mnt/data0 && -e /mnt/data1 && -e /mnt/data2 && -e /mnt/data3 ]] && { swapon | grep `realpath /dev/mapper/swap0` ; } ; } || { echo "You may need to run 'make example-run' first to mount dirs and turn on swap files." ; false ; }
	$(info Running filesystem test suites on data disks')
	cd /mnt/data0 && prove -rv ~/pjdfstest/tests
	cd /mnt/data1 && prove -rv ~/pjdfstest/tests
	cd /mnt/data2 && prove -rv ~/pjdfstest/tests
	cd /mnt/data3 && prove -rv ~/pjdfstest/tests
	$(info Testing on swap disk, now you can monitor with 'free -h')
	systemd-run --wait --property="MemoryMax=128M" --property="MemorySwapMax=infinity"  -- stress-ng --timeout 60 --vm 1 --vm-hang 0 --vm-method zero-one --vm-bytes $$(swapon | grep `realpath /dev/mapper/swap0` | awk '{ print $$3 }')

install-test-depend:
	[[ -e ~/pjdfstest/pjdfstest ]] || { cd ~/ && git clone https://github.com/pjd/pjdfstest.git && cd ~/pjdfstest && autoreconf -ifs && ./configure && make pjdfstest ; }

	which prove || { yum install -y perl-Test-Harness ; }
	which stress-ng || { yum install -y stress-ng ; }
