VERSION 	:= $(shell grep '^version' Cargo.toml | awk -F' = ' '{print $$2}' | tr -d '"')

.PHONE: help
help:
	@echo "Read README.md first"

.PHONE: update-template
update-template:
	cargo run --bin gen-template -- volume -t otp > dist/etc/volumes/otp.toml.template
	cargo run --bin gen-template -- volume -t kbs > dist/etc/volumes/kbs.toml.template
	cargo run --bin gen-template -- volume -t kms > dist/etc/volumes/kms.toml.template
	cargo run --bin gen-template -- volume -t oidc > dist/etc/volumes/oidc.toml.template
	cargo run --bin gen-template -- volume -t exec > dist/etc/volumes/exec.toml.template
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

	rsync -a --exclude target --exclude .git/modules/deps/cryptpilot-envoy ./ /tmp/cryptpilot-tarball/cryptpilot-${VERSION}/src

	tar -czf /tmp/cryptpilot-${VERSION}.tar.gz -C /tmp/cryptpilot-tarball/ cryptpilot-${VERSION}

	@echo "Tarball generated:" /tmp/cryptpilot-${VERSION}.tar.gz

.PHONE: rpm-build
rpm-build: create-tarball
	# setup build tree
	which rpmdev-setuptree || { yum install -y rpmdevtools ; }
	rpmdev-setuptree

	# copy sources
	cp /tmp/cryptpilot-${VERSION}.tar.gz ~/rpmbuild/SOURCES/

	# install build dependencies
	which yum-builddep || { yum install -y yum-utils ; }
	yum-builddep -y ./cryptpilot.spec
	
	# build 
	rpmbuild -ba ./cryptpilot.spec
	@echo "RPM package is:" ~/rpmbuild/RPMS/*/cryptpilot-*

.PHONE: rpm-build-in-an8-docker
rpm-build-in-an8-docker:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}.tar.gz ~/rpmbuild/SOURCES/

	docker run --rm -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code registry.openanolis.cn/openanolis/anolisos:8 bash -x -c "yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec"

.PHONE: rpm-build-in-an23-docker
rpm-build-in-an23-docker:
	# copy sources
	mkdir -p ~/rpmbuild/SOURCES/
	cp /tmp/cryptpilot-${VERSION}.tar.gz ~/rpmbuild/SOURCES/

	docker run --rm -v ~/rpmbuild:/root/rpmbuild -v .:/code --workdir=/code registry.openanolis.cn/openanolis/anolisos:23 bash -x -c "yum install -y rpmdevtools yum-utils; rpmdev-setuptree ; yum-builddep -y ./cryptpilot.spec ; rpmbuild -ba ./cryptpilot.spec"


.PHONE: rpm-build-in-docker
rpm-build-in-docker: rpm-build-in-an8-docker

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


.PHONE: run-test
run-test: install-test-depend
	cargo test -- --nocapture

.PHONE: install-test-depend
install-test-depend:
	[[ -e /tmp/pjdfstest/pjdfstest ]] || { cd /tmp/ && git clone https://github.com/pjd/pjdfstest.git && cd /tmp/pjdfstest && autoreconf -ifs && ./configure && make pjdfstest ; }

	which prove || { yum install -y perl-Test-Harness ; }
	which stress-ng || { yum install -y http://mirrors.openanolis.cn/anolis/8/AppStream/x86_64/os/Packages/stress-ng-0.17.08-2.0.1.an8.x86_64.rpm ; }

.PHONE: shellcheck
shellcheck:
	@command -v shellcheck >&- || { \
		echo "shellcheck not found, please installing it from https://github.com/koalaman/shellcheck/releases/download/stable/shellcheck-stable.linux.x86_64.tar.xz" ; \
	}
	find . -name '*.sh' -exec shellcheck {} \;

.PHONE: clippy
clippy:
	cargo clippy --all-targets --all-features -- -D warnings
