%global debug_package %{nil}

Name: cryptpilot
Version: 0.2.1
Release: 1%{?dist}
Summary: A utility for protecting data at rest in confidential environment
Group: Applications/System
License: ASL 2.0
URL: www.alibaba.com
Source0: https://github.com/openanolis/cryptpilot/releases/download/v%{version}/cryptpilot-%{version}.tar.gz

Source1: config

Requires: dracut lvm2 cryptsetup coreutils systemd veritysetup
# If not installed, the kbs and kms-oidc keyprovider will not work.
Recommends: confidential-data-hub
# If not installed, the AAEL will not work.
Suggests: attestation-agent

# BuildRequires: cargo, rust, protobuf-compiler
BuildRequires: protobuf-compiler
BuildRequires: perl-IPC-Cmd
BuildRequires: clang-libs
BuildRequires: cargo
BuildRequires: rust

ExclusiveArch: x86_64

%define dracut_dst %{_prefix}/lib/dracut/modules.d/91cryptpilot/


%description
A utility for protecting data at rest in confidential environment, with setting up tools and dracut module.


%prep
%setup -q -n %{name}-%{version}
# Add cargo source replacement configs
mkdir -p ~/.cargo/
cp %{SOURCE1} ~/.cargo/config


%build
ln -s `realpath %{_builddir}/%{name}-%{version}/vendor` ~/vendor
# Build cryptpilot
pushd src/
cargo install --path . --bin cryptpilot --root %{_builddir}/%{name}-%{version}/install/cryptpilot/ --locked --offline
popd
# Remove vendor
rm -f ~/vendor


%install
# Install cryptpilot
pushd src/
install -d -p %{buildroot}%{_prefix}/bin
install -p -m 755 %{_builddir}/%{name}-%{version}/install/cryptpilot/bin/cryptpilot %{buildroot}%{_prefix}/bin/cryptpilot
install -p -m 755 cryptpilot-convert.sh %{buildroot}%{_prefix}/bin/cryptpilot-convert
# Install remain stuffs
rm -rf %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/module-setup.sh %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/initrd-trigger-network-online.sh %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/initrd-wait-network-online.sh %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-before-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-after-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/initrd-wait-network-online.service %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{_prefix}/lib/systemd/system
install -d -p %{buildroot}/etc/cryptpilot
install -p -m 600 dist/etc/global.toml.template %{buildroot}/etc/cryptpilot/global.toml.template
install -p -m 600 dist/etc/fde.toml.template %{buildroot}/etc/cryptpilot/fde.toml.template
install -d -p %{buildroot}/etc/cryptpilot/volumes
install -p -m 600 dist/etc/volumes/kms.toml.template %{buildroot}/etc/cryptpilot/volumes/kms.toml.template
install -p -m 600 dist/etc/volumes/otp.toml.template %{buildroot}/etc/cryptpilot/volumes/otp.toml.template
install -p -m 600 dist/etc/volumes/kbs.toml.template %{buildroot}/etc/cryptpilot/volumes/kbs.toml.template
popd

%clean
rm -f ~/.cargo/config
rm -rf %{buildroot}

%files
%license src/LICENSE
%{_prefix}/bin/cryptpilot
%{_prefix}/bin/cryptpilot-convert
%dir /etc/cryptpilot
/etc/cryptpilot/global.toml.template
/etc/cryptpilot/fde.toml.template
%dir /etc/cryptpilot/volumes
/etc/cryptpilot/volumes/otp.toml.template
/etc/cryptpilot/volumes/kms.toml.template
/etc/cryptpilot/volumes/kbs.toml.template
%dir %{dracut_dst}
%{dracut_dst}module-setup.sh
%{dracut_dst}initrd-trigger-network-online.sh
%{dracut_dst}initrd-wait-network-online.sh
%{dracut_dst}cryptpilot-before-sysroot.service
%{dracut_dst}cryptpilot-after-sysroot.service
%{dracut_dst}initrd-wait-network-online.service

%changelog
* Mon Mar  3 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.1-1
- Add OIDC key provider Plugin
- Fix path of confidential-data-hub
- Fix failed to launch containers due to overlayfs
- Not install attestation-agent as dependency unless user install it

* Wed Feb 26 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.0-1
- RPM Build Improvements:
  * Add cryptpilot-convert to RPM package
  * Fix RPM build in Docker environments without TTY
  * Separate cargo configuration from .spec file
  * Add static binary build support
  * Update dependency names for aa and cdh

- Feature Enhancements:
  * Add OIDC Key Provider Plugin (contributed by xynnn007)
  * Support configuration loading from cloud-init
  * Add runtime measurement based on AAEL
  * Implement dynamic log level adjustment (switch to tracing framework)

- Service Optimizations:
  * Optimize automatic decryption flow during initrd stage
  * Fix systemd service dependencies
  * Add emergency shell fallback for boot failures
  * Improve console logging output

- Security Improvements:
  * Omit sensitive information in logs
  * Enforce verification of /sysroot mount source
  * Add timeout handling for KBS client

- Documentation & CI:
  * Add CI and license badges to README
  * Implement GitHub Actions RPM build workflow
  * Fix missing LICENSE file inclusion

- Bug Fixes:
  * Fix grub2-mkconfig failures under overlayfs
  * Resolve device busy issues during conversion
  * Fix builds on stable Rust versions
  * Correct Alibaba Cloud Linux 3 UEFI conversion issues


* Mon Oct 28 2024 Kun Lai <laikun@linux.alibaba.com> - 0.1.0-1
- Initial package release.
