VERSION 	:= $(shell grep '^version' Cargo.toml | awk -F' = ' '{print $$2}' | tr -d '"')

.PHONE: help
help:
	@echo "Read README.md first"

.PHONE: update-template
update-template:
	cargo run --bin gen-template -- volume -t otp > dist/etc/volumes/otp.toml.template
	cargo run --bin gen-template -- volume -t kms > dist/etc/volumes/kms.toml.template
	cargo run --bin gen-template -- volume -t kbs > dist/etc/volumes/kbs.toml.template
	cargo run --bin gen-template -- global > dist/etc/global.toml.template
	cargo run --bin gen-template -- fde > dist/etc/fde.toml.template

.PHONE: build-static
build-static:
	rustup target add x86_64-unknown-linux-musl
	cargo build --release --target x86_64-unknown-linux-musl --config target.x86_64-unknown-linux-musl.linker=\"/opt/x86-64--musl--stable-2024.05-1/bin/x86_64-buildroot-linux-musl-gcc\"

.PHONE: build
build:
	cargo build --release

.PHONE: create-tarball
create-tarball:
	rm -rf /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/ && mkdir -p /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/

	cargo vendor --manifest-path ./Cargo.toml --no-delete --versioned-dirs --respect-source-config /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/
	# remove unused files
	find /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/src/ ! -name 'lib.rs' -type f -exec rm -f {} +
	find /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/src/ ! -name 'lib.rs' -type f -exec rm -f {} +
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/lib/*.a
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/lib/*.a
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/lib/*.lib
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/lib/*.lib

	rsync -a --exclude target --exclude .git/modules/deps/cryptpilot-envoy ./ /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/src

	tar -czf /tmp/cryptpilot-${VERSION}.tar.gz -C /tmp/cryptpilot-tarball/ cryptpilot-${VERSION}

	@echo "Tarball generated:" /tmp/cryptpilot-${VERSION}.tar.gz

define CARGO_CONFIG
[source.crates-io]
replace-with = "vendored-sources"

[source."git+https://github.com/confidential-containers/guest-components.git?tag=v0.10.0"]
git = "https://github.com/confidential-containers/guest-components.git"
tag = "v0.10.0"
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
endef
export CARGO_CONFIG

.PHONE: rpm-build
rpm-build: create-tarball
	# setup build tree
	which rpmdev-setuptree || { yum install -y rpmdevtools ; }
	rpmdev-setuptree

	# copy sources
	cp /tmp/cryptpilot-${VERSION}.tar.gz ~/rpmbuild/SOURCES/
	@echo "$$CARGO_CONFIG" > ~/rpmbuild/SOURCES/config

	# install build dependencies
	which yum-builddep || { yum install -y yum-utils ; }
	yum-builddep -y ./cryptpilot.spec
	
	# build 
	rpmbuild -ba ./cryptpilot.spec
	@echo "RPM package is:" ~/rpmbuild/RPMS/*/cryptpilot-*

.PHONE: rpm-build-in-docker
rpm-build-in-docker:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}.tar.gz ~/rpmbuild/SOURCES/
	@echo "$$CARGO_CONFIG" > ~/rpmbuild/SOURCES/config

	docker run --rm -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code registry.openanolis.cn/openanolis/anolisos:8 bash -x -c "yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec"

.PHONE: rpm-install
rpm-install: rpm-build
	yum remove cryptpilot -y
	ls -t /root/rpmbuild/RPMS/x86_64/cryptpilot-*.rpm | head -n 1 | xargs rpm --install

.PHONE: update-rpm-tree
update-rpm-tree:
	# copy sources
	rm -f ../rpm-tree-cryptpilot/cryptpilot-*.tar.gz
	cp /tmp/cryptpilot-${VERSION}.tar.gz ../rpm-tree-cryptpilot/
	cp ./cryptpilot.spec ../rpm-tree-cryptpilot/
	@echo "$$CARGO_CONFIG" > ../rpm-tree-cryptpilot/config

.PHONE: example-prepare
example-prepare: example-clean
	parted --script /dev/nvme1n1 \
            mktable gpt \
            mkpart data0 0% 10% \
            mkpart data1 10% 20% \
            mkpart data2 20% 30% \
            mkpart data3 30% 40% \
            mkpart data4 40% 50% \
            mkpart swap0 50% 100%
	partprobe
	rm -rf /etc/cryptpilot/
	rsync -avp --exclude=fde.toml ./examples/ /etc/cryptpilot/

.PHONE: example-clean
example-clean:
	! mountpoint -q /mnt/data0 || umount /mnt/data0 ; [ ! -e /dev/mapper/data0 ] || dmsetup remove data0 ; [ ! -e /dev/mapper/data0_dif ] || dmsetup remove data0_dif ; dd if=/dev/urandom of=/dev/nvme1n1p1 count=16 seek=0 bs=4096 ;
	! mountpoint -q /mnt/data1 || umount /mnt/data1 ; [ ! -e /dev/mapper/data1 ] || dmsetup remove data1 ; [ ! -e /dev/mapper/data1_dif ] || dmsetup remove data1_dif ; dd if=/dev/urandom of=/dev/nvme1n1p2 count=16 seek=0 bs=4096 ;
	! mountpoint -q /mnt/data2 || umount /mnt/data2 ; [ ! -e /dev/mapper/data2 ] || dmsetup remove data2 ; [ ! -e /dev/mapper/data2_dif ] || dmsetup remove data2_dif ; dd if=/dev/urandom of=/dev/nvme1n1p3 count=16 seek=0 bs=4096 ;
	! mountpoint -q /mnt/data3 || umount /mnt/data3 ; [ ! -e /dev/mapper/data3 ] || dmsetup remove data3 ; [ ! -e /dev/mapper/data3_dif ] || dmsetup remove data3_dif ; dd if=/dev/urandom of=/dev/nvme1n1p4 count=16 seek=0 bs=4096 ;
	! mountpoint -q /mnt/data4 || umount /mnt/data4 ; [ ! -e /dev/mapper/data4 ] || dmsetup remove data4 ; [ ! -e /dev/mapper/data4_dif ] || dmsetup remove data4_dif ; dd if=/dev/urandom of=/dev/nvme1n1p5 count=16 seek=0 bs=4096 ;
	! { swapon | grep swap0 ; } || swapoff /dev/mapper/swap0 ; [ ! -e /dev/mapper/swap0 ] || dmsetup remove swap0 ; [ ! -e /dev/mapper/swap0_dif ] || dmsetup remove swap0_dif ; dd if=/dev/urandom of=/dev/nvme1n1p6 count=16 seek=0 bs=4096 ;

.PHONE: example-run
example-run: example-clean
	cryptpilot init data0 -y && cryptpilot open data0 && mkdir -p /mnt/data0 && mount -t ext4 /dev/mapper/data0 /mnt/data0
	cryptpilot init data1 -y && cryptpilot open data1 && mkdir -p /mnt/data1 && mount -t ext4 /dev/mapper/data1 /mnt/data1 && echo -n test > /mnt/data1/testfile
	umount /mnt/data1 && cryptpilot open data1 && mkdir -p /mnt/data1 && mount -t ext4 /dev/mapper/data1 /mnt/data1 && [[ `cat /mnt/data1/testfile` == "test" ]]
	cryptpilot init data2 -y && cryptpilot open data2 && mkdir -p /mnt/data2 && mount -t ext4 /dev/mapper/data2 /mnt/data2
	cryptpilot init data3 -y && cryptpilot open data3 && mkdir -p /mnt/data3 && mount -t xfs /dev/mapper/data3 /mnt/data3
	cryptpilot init data4 -y && cryptpilot open data4 && mkdir -p /mnt/data4 && mount -t xfs /dev/mapper/data4 /mnt/data4
	cryptpilot init swap0 -y && cryptpilot open swap0 && swapon /dev/mapper/swap0
	$(info All is done. Now you can check with 'findmnt' and 'swapon')

.PHONE: example-run-test
example-run-test: install-test-depend
	{ [[ -e /mnt/data0 && -e /mnt/data1 && -e /mnt/data2 && -e /mnt/data3 && -e /mnt/data4 ]] && { swapon | grep `realpath /dev/mapper/swap0` ; } ; } || { echo "You may need to run 'make example-run' first to mount dirs and turn on swap files." ; false ; }
	$(info Running filesystem test suites on data disks')
	cd /mnt/data0 && prove -rv ~/pjdfstest/tests
	cd /mnt/data1 && prove -rv ~/pjdfstest/tests
	cd /mnt/data2 && prove -rv ~/pjdfstest/tests
	cd /mnt/data3 && prove -rv ~/pjdfstest/tests
	cd /mnt/data4 && prove -rv ~/pjdfstest/tests
	$(info Testing on swap disk, now you can monitor with 'free -h')
	systemd-run --wait --property="MemoryMax=128M" --property="MemorySwapMax=infinity"  -- stress-ng --timeout 60 --vm 1 --vm-hang 0 --vm-method zero-one --vm-bytes $$(swapon | grep `realpath /dev/mapper/swap0` | awk '{ print $$3 }')

.PHONE: install-test-depend
install-test-depend:
	[[ -e ~/pjdfstest/pjdfstest ]] || { cd ~/ && git clone https://github.com/pjd/pjdfstest.git && cd ~/pjdfstest && autoreconf -ifs && ./configure && make pjdfstest ; }

	which prove || { yum install -y perl-Test-Harness ; }
	which stress-ng || { yum install -y stress-ng ; }
