Name: cryptpilot
Version: 0.1.0
Release: 1%{?dist}
Summary: A utility for protecting data at rest in confidential environment
Group: Applications/System
License: Alibaba
URL: www.alibaba.com
Requires: dracut lvm2 cryptsetup coreutils systemd

# BuildRequires: cargo, rust, protobuf-compiler
BuildRequires: protobuf-compiler
BuildArch: x86_64

%define dracut_dst %{_prefix}/lib/dracut/modules.d/91crypt-luks/

%description
A utility for protecting data at rest in confidential environment, with setting up tools and dracut module.

%prep
# https://stackoverflow.com/a/48484540/15011229
find . -mindepth 1 -delete
cp -af %{expand:%%(pwd)}/. .

%build
rustup target add x86_64-unknown-linux-musl
# cargo build --release --target x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl --config target.x86_64-unknown-linux-musl.linker=\"/opt/x86-64--musl--stable-2024.05-1/bin/x86_64-buildroot-linux-musl-gcc\"

%install
mkdir -p %{buildroot}%{_prefix}/bin
cp target/x86_64-unknown-linux-musl/release/cryptpilot %{buildroot}%{_prefix}/bin/
chmod 755 %{buildroot}%{_prefix}/bin/cryptpilot
strip %{buildroot}%{_prefix}/bin/cryptpilot
rm -rf %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{dracut_dst}
# install -p -m 755 dist/dracut/modules.d/91luks-agent/module-setup.sh %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{_prefix}/lib/systemd/system
install -p -m 644 dist/cryptpilot.service %{buildroot}%{_prefix}/lib/systemd/system/cryptpilot.service
install -d -p %{buildroot}/etc/cryptpilot
install -p -m 600 dist/etc/cryptpilot.toml %{buildroot}/etc/cryptpilot/cryptpilot.toml
install -d -p %{buildroot}/etc/cryptpilot/volumes
install -p -m 600 dist/etc/volumes/kms.toml.template %{buildroot}/etc/cryptpilot/volumes/kms.toml.template
install -p -m 600 dist/etc/volumes/otp.toml.template %{buildroot}/etc/cryptpilot/volumes/otp.toml.template

%clean
rm -rf %{buildroot}

%files
%{_prefix}/bin/cryptpilot
%{_prefix}/lib/systemd/system/cryptpilot.service
/etc/cryptpilot/cryptpilot.toml
/etc/cryptpilot/volumes
/etc/cryptpilot/volumes/kms.toml.template
/etc/cryptpilot/volumes/otp.toml.template
# %{dracut_dst}module-setup.sh

%post
systemctl daemon-reload

%preun
if [ $1 == 0 ]; then #uninstall
  systemctl unmask %{name}.service
  systemctl stop %{name}.service
  systemctl disable %{name}.service
fi

%postun
if [ $1 == 0 ]; then #uninstall
  systemctl daemon-reload
  systemctl reset-failed
fi

%changelog
* Mon Oct 28 2024 Kun Lai <laikun@linux.alibaba.com> - 0.1.0-1
- Initial package release.
