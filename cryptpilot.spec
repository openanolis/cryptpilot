%global debug_package %{nil}
%define release_num 1

Name: cryptpilot
Version: 0.2.6
Release: %{release_num}%{?dist}
Summary: A utility for protecting data at rest in confidential environment
Group: Applications/System
License: Apache-2.0
URL: https://www.alibaba.com
Source0: https://github.com/openanolis/cryptpilot/releases/download/v%{version}-%{release_num}/cryptpilot-%{version}.tar.gz

Requires: dracut
Requires: lvm2
Requires: cryptsetup
Requires: coreutils
Requires: systemd
Requires: systemd-udev
Requires: veritysetup
Requires: device-mapper-libs
Requires: kmod
# mkfs.vfat
Requires: dosfstools
# mkfs.xfs
Requires: xfsprogs
# mkfs.ext4
Requires: e2fsprogs
# swapon
Requires: util-linux
# qemu-nbd
Requires: qemu-img
Requires: file

# If not installed, the kbs and kms-oidc keyprovider will not work.
Recommends: confidential-data-hub
# If not installed, the AAEL will not work.
Suggests: attestation-agent

BuildRequires: protobuf-compiler
BuildRequires: perl-IPC-Cmd
BuildRequires: clang-libs
BuildRequires: cargo
BuildRequires: rust
BuildRequires: clang
BuildRequires: device-mapper-devel

ExclusiveArch: x86_64

%define dracut_dst %{_prefix}/lib/dracut/modules.d/91cryptpilot/


%description
A utility for protecting data at rest in confidential environment, with setting up tools and dracut module.


%prep
%setup -q -n %{name}-%{version}


%build
# Build cryptpilot
pushd src/
cargo install --path . --bin cryptpilot --root %{_builddir}/%{name}-%{version}/install/cryptpilot/ --locked --offline
popd


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
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-before-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-after-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/initrd-wait-network-online.service %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{_prefix}/lib/systemd/system
install -p -m 644 dist/systemd/cryptpilot.service %{buildroot}%{_prefix}/lib/systemd/system/cryptpilot.service
install -d -p %{buildroot}/etc/cryptpilot
install -p -m 600 dist/etc/global.toml.template %{buildroot}/etc/cryptpilot/global.toml.template
install -p -m 600 dist/etc/fde.toml.template %{buildroot}/etc/cryptpilot/fde.toml.template
install -d -p %{buildroot}/etc/cryptpilot/volumes
install -p -m 600 dist/etc/volumes/otp.toml.template %{buildroot}/etc/cryptpilot/volumes/otp.toml.template
install -p -m 600 dist/etc/volumes/kbs.toml.template %{buildroot}/etc/cryptpilot/volumes/kbs.toml.template
install -p -m 600 dist/etc/volumes/kms.toml.template %{buildroot}/etc/cryptpilot/volumes/kms.toml.template
install -p -m 600 dist/etc/volumes/oidc.toml.template %{buildroot}/etc/cryptpilot/volumes/oidc.toml.template
install -p -m 600 dist/etc/volumes/exec.toml.template %{buildroot}/etc/cryptpilot/volumes/exec.toml.template
install -d -p %{buildroot}/usr/share/cryptpilot
install -p -m 644 dist/usr/share/cryptpilot/policy.rego %{buildroot}/usr/share/cryptpilot/policy.rego
install -d -p %{buildroot}/usr/lib/udev/rules.d
install -p -m 644 dist/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules %{buildroot}/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules
popd


%post
# Reload systemd manager configuration to pick up new/updated service files
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || :
fi

# Reload udev rules to apply new device filtering rules
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || :
fi


%clean
rm -rf %{buildroot}


%files
%license src/LICENSE
%{_prefix}/bin/cryptpilot
%{_prefix}/bin/cryptpilot-convert
%{_prefix}/lib/systemd/system/cryptpilot.service
%dir /etc/cryptpilot
/etc/cryptpilot/global.toml.template
/etc/cryptpilot/fde.toml.template
%dir /etc/cryptpilot/volumes
/etc/cryptpilot/volumes/otp.toml.template
/etc/cryptpilot/volumes/kbs.toml.template
/etc/cryptpilot/volumes/kms.toml.template
/etc/cryptpilot/volumes/oidc.toml.template
/etc/cryptpilot/volumes/exec.toml.template
%dir %{dracut_dst}
%{dracut_dst}module-setup.sh
%{dracut_dst}initrd-trigger-network-online.sh
%{dracut_dst}initrd-wait-network-online.sh
%{dracut_dst}cryptpilot-fde-before-sysroot.service
%{dracut_dst}cryptpilot-fde-after-sysroot.service
%{dracut_dst}initrd-wait-network-online.service
%dir /usr/share/cryptpilot
/usr/share/cryptpilot/policy.rego
/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules


%preun
if [ $1 == 0 ]; then #uninstall
  systemctl unmask cryptpilot.service
  systemctl stop cryptpilot.service
  systemctl disable cryptpilot.service
fi


%postun
if [ $1 == 0 ]; then #uninstall
  systemctl daemon-reload
  systemctl reset-failed
fi


%changelog
* Mon Sep 15 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.6-1
- cryptpilot & FDE Enhancements:
  * Redirect all logs to stderr for consistent logging behavior
  * Fix kernel module loading failures on specific systems
  * Resolve race condition where block devices appear after cryptpilot service start
  * Add passphrase validation before unlocking encrypted volumes
  * Enforce root=/dev/mapper/rootfs in kernel command line via force override
  * Fix boot partition detection logic
  * Improve network stability during early boot

- cryptpilot-convert Improvements:
  * Speed up conversion of large disk images
  * Replace yum --installroot with chroot-based package installation
  * Add --boot_part_size parameter to customize boot partition size
  * Add --rootfs-part-num to set root filesystem partition number
  * Enhance EFI and rootfs partition detection based on content inspection
  * Fix access failure after partition creation
  * Correctly detect default kernel in multi-kernel systems
  * Fix encrypted image creation for AnolisOS-23.3-x86_64.qcow2
  * Prevent repeated mounting of EFI/boot partitions by adding noauto,nofail to fstab
  * Optimize e2fsck execution logic
  * Add boot partition pre-check functionality
  * Improve compatibility with various disk partition layouts
  * Force override / mount source in /etc/fstab
  * Add colored logging output for better readability
  * Enable support for AnolisOS 23.3 and Alinux3 software installation via yum

- show-reference-value Updates:
  * Support SM3 hash algorithm for reference value calculation
  * Generate reference values for GRUB, kernel, cmdline, and initrd
  * Fix kernel path generation issues
  * Support multiple GRUB and shim binaries in /boot
  * Suppress noisy mount error messages
  * Remove irrelevant print output during command execution
  * Fix failure in cleaning up DM devices from NBD instances
  * Remove redundant 'tdx' prefix from output

- Container & OverlayFS Fixes:
  * Fix "not supported as upperdir" error in Docker
  * Resolve "overlay is not supported over overlayfs" error in Podman

- FDE & Configuration Changes:
  * Change load_config content format from JSON object to hex hash value

- Infrastructure & Compatibility:
  * Add Aliyun IMDS availability check before fetching instance config

* Mon Jul  7 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.5-5
- fde: sync time to system before call cdh if run in aliyun ecs.
- fde: add timeout fetching config from cloudinit.

* Wed Jul  2 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.5-4
- Fix "Failed to load kernel module 'nbd'" when used in docker container.

* Mon Jun 30 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.5-3
- cryptpilot-convert: fix occasional "device or resource busy" error when rootfs encryption is enabled


* Thu Jun 12 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.5-2
- cryptpilot-convert: fix failed checking free nbd device when no nbd kernel module avaliable
- cryptpilot.spec: add missing requires for file package
- cmd/open: add checking passphrase before open the volume
- fs/nbd.rs: change udev rule path to volatile runtime directory /run/udev/rules.d


* Thu Jun 12 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.5-1
- Add "cryptpilot config check" command to check if the config is valid
- Add support to specify more than one volume name to open/init/close command
- Remove the "config dump" command
- Change short form of --config-dir from -d to -c
- Add the "fde show-reference-value" and "fde dump-config" command
- Add --rootfs-no-encryption option to cryptpilot-convert to make disk with rootfs volume unencrypted

* Fri May 23 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.4-1
- Fix broken FDE due to wrong dm-verity kernel module name

* Mon Apr 28 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.3-1
- Add new key provider plugin type "exec"
- Fix wrong lvm part size in disk image converted by cryptpilot-convert
- Change systemd service in initrd from RequiredBy to WantedBy for weaker dependency

* Tue Apr 15 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.2-2
- Update source code git hash from c2257859 to ea829279

* Mon Mar 31 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.2-1
- Fix TLS support in dracut initrd
- Add RUST_LOG environment variable to control log level
- Fix udev env SYSTEMD_READY=0 when integrity is on

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
