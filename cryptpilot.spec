%global debug_package %{nil}

Name: cryptpilot
Version: 0.1.0
Release: 1%{?dist}
Summary: A utility for protecting data at rest in confidential environment
Group: Applications/System
License: ASL 2.0
URL: www.alibaba.com
Source0: https://github.com/openanolis/cryptpilot/releases/download/v%{version}/cryptpilot-%{version}.tar.gz

Source1: config

Requires: dracut lvm2 cryptsetup coreutils systemd veritysetup
Recommends: attestation-agent confidential-data-hub

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
strip %{_builddir}/%{name}-%{version}/install/cryptpilot/bin/cryptpilot
popd
# Remove vendor
rm -f ~/vendor


%install
# Install cryptpilot
pushd src/
mkdir -p %{buildroot}%{_prefix}/bin
mkdir -p %{buildroot}%{_prefix}/lib/cryptpilot/bin/
cp %{_builddir}/%{name}-%{version}/install/cryptpilot/bin/cryptpilot %{buildroot}%{_prefix}/lib/cryptpilot/bin/
ln -s %{_prefix}/lib/cryptpilot/bin/cryptpilot %{buildroot}%{_prefix}/bin/
chmod 755 %{buildroot}%{_prefix}/lib/cryptpilot/bin/cryptpilot
strip %{buildroot}%{_prefix}/lib/cryptpilot/bin/cryptpilot
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
%dir %{_prefix}/lib/cryptpilot/bin
%{_prefix}/lib/cryptpilot/bin/cryptpilot
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
* Mon Oct 28 2024 Kun Lai <laikun@linux.alibaba.com> - 0.1.0-1
- Initial package release.
