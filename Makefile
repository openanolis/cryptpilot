VERSION 	:= $(shell grep '^version' Cargo.toml | awk -F' = ' '{print $$2}' | tr -d '"')
RELEASE_NUM := 1

ARCH := $(shell uname -m)

# Map x86_64 to amd64 and aarch64 to arm64 for DEB packages
ifeq ($(ARCH),x86_64)
	DEB_ARCH := amd64
	MUSL_PATH_ARCH := x86-64
else ifeq ($(ARCH),aarch64)
	DEB_ARCH := arm64
	MUSL_PATH_ARCH := aarch64
else
	DEB_ARCH := $(ARCH)
	MUSL_PATH_ARCH := $(ARCH)
endif

.PHONE: help
help:
	@echo "Read README.md first"

.PHONE: update-template
update-template:
	# Generate volume templates using cryptpilot-crypt
	cargo run --bin crypt-gen-template --package cryptpilot-crypt -- -t otp > dist/etc/volumes/otp.toml.template
	cargo run --bin crypt-gen-template --package cryptpilot-crypt -- -t kbs > dist/etc/volumes/kbs.toml.template
	cargo run --bin crypt-gen-template --package cryptpilot-crypt -- -t kms > dist/etc/volumes/kms.toml.template
	cargo run --bin crypt-gen-template --package cryptpilot-crypt -- -t oidc > dist/etc/volumes/oidc.toml.template
	cargo run --bin crypt-gen-template --package cryptpilot-crypt -- -t exec > dist/etc/volumes/exec.toml.template
	# Generate FDE templates using cryptpilot-fde
	cargo run --bin fde-gen-template --package cryptpilot-fde -- global > dist/etc/global.toml.template
	cargo run --bin fde-gen-template --package cryptpilot-fde -- fde > dist/etc/fde.toml.template

.PHONE: build-static
build-static:
	rustup target add $(ARCH)-unknown-linux-musl
	cargo build --release --target $(ARCH)-unknown-linux-musl --config target.$(ARCH)-unknown-linux-musl.linker=\"/opt/$(MUSL_PATH_ARCH)--musl--stable-2024.05-1/bin/$(ARCH)-buildroot-linux-musl-gcc\"

.PHONE: build
build:
	cargo build --release

.PHONE: create-tarball
create-tarball:
	rm -rf /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/ && mkdir -p /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/

	mkdir -p /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/.cargo/
	cargo vendor --locked --manifest-path ./Cargo.toml --no-delete --versioned-dirs --respect-source-config /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor// | tee /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/.cargo/config.toml

	sed -i 's;^.*directory = .*/vendor/.*$$;directory = "vendor";g' /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/.cargo/config.toml

	# sanity check on cargo vendor
	@grep "source.crates-io" /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/.cargo/config.toml >/dev/null || (echo "cargo vendor failed, please check /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/.cargo/config.toml"; exit 1)

	# remove unused files
	find /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/src/ ! -name 'lib.rs' -type f -exec rm -f {} +
	find /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/src/ ! -name 'lib.rs' -type f -exec rm -f {} +
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/lib/*.a
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/lib/*.a
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/winapi*/lib/*.lib
	rm -fr /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/vendor/windows*/lib/*.lib

	# copy source code to src/
	git clone --no-hardlinks . /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/src/
	cd /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/src && git clean -xdf

	tar -czf /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz -C /tmp/cryptpilot-tarball/ cryptpilot-${VERSION}

	@echo "Tarball generated:" /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz

.PHONE: rpm-build
rpm-build:
	# setup build tree
	which rpmdev-setuptree || { yum install -y rpmdevtools ; }
	rpmdev-setuptree

	# copy sources
	cp /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz ~/rpmbuild/SOURCES/

	# install build dependencies
	which yum-builddep || { yum install -y yum-utils ; }
	yum-builddep -y --skip-unavailable ./cryptpilot.spec
	
	# build 
	rpmbuild -ba ./cryptpilot.spec --define 'with_rustup 1'
	@echo "RPM packages are:"
	@ls -1 ~/rpmbuild/RPMS/*/cryptpilot-[0-9]*.rpm ~/rpmbuild/RPMS/*/cryptpilot-fde-[0-9]*.rpm ~/rpmbuild/RPMS/*/cryptpilot-crypt-[0-9]*.rpm ~/rpmbuild/RPMS/*/cryptpilot-verity-[0-9]*.rpm 2>/dev/null || true

.PHONE: rpm-build-in-al3-docker
rpm-build-in-al3-docker:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz ~/rpmbuild/SOURCES/

	docker run --rm -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code alibaba-cloud-linux-3-registry.cn-hangzhou.cr.aliyuncs.com/alinux3/alinux3:latest bash -x -c "sed -i -E 's|https?://mirrors.cloud.aliyuncs.com/|https://mirrors.aliyun.com/|g' /etc/yum.repos.d/*.repo ; curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none ; source \"\$$HOME/.cargo/env\" ; yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y --skip-unavailable ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec --define 'with_rustup 1'"

.PHONE: rpm-build-in-an23-docker
rpm-build-in-an23-docker:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz ~/rpmbuild/SOURCES/

	docker run --rm -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code registry.openanolis.cn/openanolis/anolisos:23 bash -x -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none ; source \"\$$HOME/.cargo/env\" ; yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y --skip-unavailable ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec --define 'with_rustup 1'"

.PHONE: rpm-build-in-docker
rpm-build-in-docker: rpm-build-in-al3-docker

.PHONE: rpm-build-in-docker-aarch64
rpm-build-in-docker-aarch64:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz ~/rpmbuild/SOURCES/

	docker run --rm --platform linux/arm64 -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code alibaba-cloud-linux-3-registry.cn-hangzhou.cr.aliyuncs.com/alinux3/alinux3:latest bash -x -c "sed -i -E 's|https?://mirrors.cloud.aliyuncs.com/|https://mirrors.aliyun.com/|g' /etc/yum.repos.d/*.repo ; curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none ; source \"\$$HOME/.cargo/env\" ; yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y --skip-unavailable ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec --define 'with_rustup 1'"

.PHONE: rpm-install
rpm-install: rpm-build
	yum remove cryptpilot cryptpilot-fde cryptpilot-crypt cryptpilot-verity -y || true
	ls -t /root/rpmbuild/RPMS/$(ARCH)/cryptpilot-[0-9]*.rpm | head -n 1 | xargs rpm --install
	ls -t /root/rpmbuild/RPMS/$(ARCH)/cryptpilot-fde-*.rpm | head -n 1 | xargs rpm --install
	ls -t /root/rpmbuild/RPMS/$(ARCH)/cryptpilot-crypt-*.rpm | head -n 1 | xargs rpm --install
	ls -t /root/rpmbuild/RPMS/$(ARCH)/cryptpilot-verity-*.rpm | head -n 1 | xargs rpm --install

.PHONE: update-rpm-tree
update-rpm-tree:
	# copy sources
	rm -f ../rpm-tree-cryptpilot/cryptpilot-*.tar.gz
	cp /tmp/cryptpilot-${VERSION}-vendored-source.tar.gz ../rpm-tree-cryptpilot/
	cp ./cryptpilot.spec ../rpm-tree-cryptpilot/


.PHONY: deb-build
deb-build:
	dpkg-buildpackage -us -uc -b
	@echo "DEB packages are in parent directory:"
	@ls -lh ../cryptpilot*.deb 2>/dev/null || true

.PHONY: deb-install
deb-install: deb-build
	apt-get remove -y cryptpilot cryptpilot-fde cryptpilot-crypt cryptpilot-verity || true
	dpkg -i ../cryptpilot-verity_*.deb ../cryptpilot-fde_*.deb ../cryptpilot-crypt_*.deb ../cryptpilot_*.deb
	apt-get install -f -y

.PHONE: run-test
run-test: install-test-depend
	cargo test -- --nocapture

.PHONE: install-test-depend
install-test-depend:
	[[ -e /tmp/pjdfstest/pjdfstest ]] || { cd /tmp/ && git clone https://github.com/pjd/pjdfstest.git && cd /tmp/pjdfstest && autoreconf -ifs && ./configure && make pjdfstest ; }

	which prove || { yum install -y perl-Test-Harness ; }
	which stress-ng || { yum install -y http://mirrors.openanolis.cn/anolis/8/AppStream/$(ARCH)/os/Packages/stress-ng-0.17.08-2.0.1.an8.$(ARCH).rpm ; }

.PHONE: shellcheck
shellcheck:
	@command -v shellcheck >&- || { \
		echo "shellcheck not found, please installing it from https://github.com/koalaman/shellcheck/releases/download/stable/shellcheck-stable.linux.$(ARCH).tar.xz" ; \
	}
	find . -name '*.sh' -exec shellcheck {} \;

.PHONE: clippy
clippy:
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: deb-build-in-docker
deb-build-in-docker:
	mkdir -p ~/deb-packages/
	docker run --rm \
		-v ~/deb-packages:/root/deb-packages \
		-v .:/code \
		--workdir=/code \
		ubuntu:24.04 \
		bash -x -c "\
			apt-get update && \
			apt-get install -y build-essential debhelper devscripts curl cmake \
				protobuf-compiler libcryptsetup-dev libdevmapper-dev libfuse3-dev \
				clang pkg-config && \
			curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.82.0 && \
			source \"\$$HOME/.cargo/env\" && \
			dpkg-buildpackage -us -uc -b && \
			cp ../*.deb /root/deb-packages/"
	@echo "DEB packages are in ~/deb-packages/"
	@ls -lh ~/deb-packages/*.deb 2>/dev/null || true


