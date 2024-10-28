Name: cryptpilot
Version: 0.1.0
Release: 1%{?dist}
Summary: A utility for protecting data at rest in confidential environment
Group: Applications/System
License: Alibaba
URL: www.alibaba.com
Requires: dracut lvm2 cryptsetup

BuildRequires: cargo, rust
BuildArch: x86_64

%define dracut_dst %{_prefix}/lib/dracut/modules.d/91crypt-luks/

%description
A utility for protecting data at rest in confidential environment, with setting up tools and dracut module.

%prep
# https://stackoverflow.com/a/48484540/15011229
find . -mindepth 1 -delete
cp -af %{expand:%%(pwd)}/. .
tree -L 3 ../

%build
pwd
tree -L 3 ../
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl

%install
mkdir -p %{buildroot}%{_prefix}/bin
cp target/x86_64-unknown-linux-musl/release/cryptpilot %{buildroot}%{_prefix}/bin/
chmod 755 %{buildroot}%{_prefix}/bin/cryptpilot
rm -rf %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{dracut_dst}
install -p -m 755 dracut/modules.d/91luks-agent/module-setup.sh %{buildroot}%{dracut_dst}

%clean
rm -rf %{buildroot}

%files
%{_prefix}/bin/cryptpilot
%{dracut_dst}module-setup.sh

%changelog
* Mon Oct 28 2024 Kun Lai <laikun@linux.alibaba.com> - 0.1.0-1
- Initial package release.
