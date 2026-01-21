%global debug_package %{nil}
%define release_num 1

Name: cryptpilot
Version: 0.3.0
Release: %{release_num}%{?dist}
Summary: Full-disk encryption and data protection tool for confidential computing
Group: Applications/System
License: Apache-2.0
URL: https://www.alibaba.com
Source0: https://github.com/openanolis/cryptpilot/releases/download/v%{version}/cryptpilot-%{version}-vendored-source.tar.gz

# Shared BuildRequires for all packages
%{!?with_rustup:%global use_system_rust 1}
%if 0%{?use_system_rust}
BuildRequires: cargo >= 1.82.0
BuildRequires: rust >= 1.82.0
%endif
# cmake is required by flatc crate for building FlatBuffers compiler
BuildRequires: cmake


%define dracut_dst %{_prefix}/lib/dracut/modules.d/91cryptpilot/

# Build dependencies (shared across all packages)
BuildRequires: protobuf-compiler
BuildRequires: cryptsetup-devel
BuildRequires: perl-IPC-Cmd
BuildRequires: clang-libs
BuildRequires: clang
BuildRequires: device-mapper-devel

# Runtime dependencies for cryptpilot (main package)
Requires: cryptpilot-fde = %{version}-%{release}
Requires: cryptpilot-crypt = %{version}-%{release}
Requires: cryptpilot-verity = %{version}-%{release}

%description
Cryptpilot is a full-disk encryption utility for protecting data at rest in confidential computing environments.
It provides LUKS2-based encryption for rootfs and data volumes, integrates with dracut for early-boot decryption,
and supports multiple key providers including TPM2, KBS (Key Broker Service), KMS, OIDC, OTP, and custom exec plugins.
Includes tools for disk image conversion (cryptpilot-convert) and enhancement (cryptpilot-enhance).

%package -n cryptpilot-verity
Summary: Integrity measurement tool for directory trees
Group: Applications/System
License: Apache-2.0

# Runtime dependencies for cryptpilot-verity
Requires: fuse3-libs
Requires: fuse3

Obsoletes: cryptpilot < %{version}-%{release}

BuildRequires: fuse3-devel

%description -n cryptpilot-verity
Cryptpilot-verity is an integrity measurement tool for directory trees in confidential computing environments.

%package -n cryptpilot-fde
Summary: Full-disk encryption tooling for system volumes
Group: Applications/System
License: Apache-2.0

# FDE runtime dependencies
Requires: cryptsetup
Requires: cryptsetup-libs
Requires: device-mapper-libs
Requires: kmod
Requires: coreutils
Requires: systemd
Requires: systemd-udev
Requires: dracut
Requires: lvm2
# Used by cryptpilot-convert for disk conversion
Requires: qemu-img
# Used by cryptpilot-enhance for partition manipulation
Requires: util-linux
Requires: veritysetup
# Filesystem tools for volume formatting
# mkfs.vfat
Recommends: dosfstools
# mkfs.xfs
Recommends: xfsprogs
# mkfs.ext4
Recommends: e2fsprogs
# swapon for swap volumes
Requires: util-linux
# If not installed, the kbs and kms-oidc keyprovider will not work.
Suggests: confidential-data-hub
# If not installed, the AAEL will not work.
Suggests: attestation-agent

Obsoletes: cryptpilot < %{version}-%{release}

%description -n cryptpilot-fde
Cryptpilot-fde provides system disk encryption and boot-time processing for full-disk encryption setups.
Includes cryptpilot-convert for disk image conversion and cryptpilot-enhance for FDE enhancement.

%package -n cryptpilot-crypt
Summary: Data volume encryption tooling
Group: Applications/System
License: Apache-2.0

# Crypt runtime dependencies
Requires: cryptsetup
Requires: cryptsetup-libs
Requires: device-mapper-libs
Requires: kmod
Requires: coreutils
Requires: systemd
# Filesystem tools for volume formatting
# mkfs.vfat
Recommends: dosfstools
# mkfs.xfs
Recommends: xfsprogs
# mkfs.ext4
Recommends: e2fsprogs
# swapon for swap volumes
Requires: util-linux

# If not installed, the kbs and kms-oidc keyprovider will not work.
Suggests: confidential-data-hub
# If not installed, the AAEL will not work.
Suggests: attestation-agent

Obsoletes: cryptpilot < %{version}-%{release}

%description -n cryptpilot-crypt
Cryptpilot-crypt provides data volume encryption and automatic volume management for confidential computing environments.

%prep
%setup -q -n %{name}-%{version}


%build
# Build cryptpilot-fde
pushd src/cryptpilot-fde/
cargo install --path . --bin cryptpilot-fde --root %{_builddir}/%{name}-%{version}/install/cryptpilot-fde/ --locked --offline
popd

# Build cryptpilot-crypt
pushd src/cryptpilot-crypt/
cargo install --path . --bin cryptpilot-crypt --root %{_builddir}/%{name}-%{version}/install/cryptpilot-crypt/ --locked --offline
popd

# Build cryptpilot-verity
pushd src/cryptpilot-verity/
cargo install --path . --bin cryptpilot-verity --root %{_builddir}/%{name}-%{version}/install/cryptpilot-verity/ --locked --offline
popd


%install
# Install cryptpilot-fde
pushd src/
install -d -p %{buildroot}%{_prefix}/bin
install -p -m 755 %{_builddir}/%{name}-%{version}/install/cryptpilot-fde/bin/cryptpilot-fde %{buildroot}%{_prefix}/bin/cryptpilot-fde
# Install FDE enhancement scripts
install -p -m 755 cryptpilot-convert.sh %{buildroot}%{_prefix}/bin/cryptpilot-convert
install -p -m 755 cryptpilot-enhance.sh %{buildroot}%{_prefix}/bin/cryptpilot-enhance

# Install cryptpilot-crypt
install -p -m 755 %{_builddir}/%{name}-%{version}/install/cryptpilot-crypt/bin/cryptpilot-crypt %{buildroot}%{_prefix}/bin/cryptpilot-crypt

# Install cryptpilot-verity
install -p -m 755 %{_builddir}/%{name}-%{version}/install/cryptpilot-verity/bin/cryptpilot-verity %{buildroot}%{_prefix}/bin/cryptpilot-verity
# Install remain stuffs
rm -rf %{buildroot}%{dracut_dst}
install -d -p %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/module-setup.sh %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/initrd-trigger-network-online.sh %{buildroot}%{dracut_dst}
install -p -m 755 dist/dracut/modules.d/91cryptpilot/initrd-wait-network-online.sh %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-before-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/cryptpilot-fde-after-sysroot.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/initrd-wait-network-online.service %{buildroot}%{dracut_dst}
install -p -m 644 dist/dracut/modules.d/91cryptpilot/lvm.conf %{buildroot}%{dracut_dst}
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


%clean
rm -rf %{buildroot}


%files
%license src/LICENSE

%files -n cryptpilot-verity
%license src/LICENSE
%{_prefix}/bin/cryptpilot-verity

%files -n cryptpilot-fde
%license src/LICENSE
%{_prefix}/bin/cryptpilot-fde
%{_prefix}/bin/cryptpilot-convert
%{_prefix}/bin/cryptpilot-enhance
# FDE configuration templates
%dir /etc/cryptpilot
/etc/cryptpilot/global.toml.template
/etc/cryptpilot/fde.toml.template
# Dracut integration for FDE boot
%dir %{dracut_dst}
%{dracut_dst}module-setup.sh
%{dracut_dst}initrd-trigger-network-online.sh
%{dracut_dst}initrd-wait-network-online.sh
%{dracut_dst}cryptpilot-fde-before-sysroot.service
%{dracut_dst}cryptpilot-fde-after-sysroot.service
%{dracut_dst}initrd-wait-network-online.service
%{dracut_dst}lvm.conf
# Policy for FDE
%dir /usr/share/cryptpilot
/usr/share/cryptpilot/policy.rego
# Udev rules for device hiding
/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules

%post -n cryptpilot-fde
# Reload udev rules to apply new device filtering rules
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || :
fi

%files -n cryptpilot-crypt
%license src/LICENSE
%{_prefix}/bin/cryptpilot-crypt
# Systemd service for volume management
%{_prefix}/lib/systemd/system/cryptpilot.service
# Volume configuration templates
%dir /etc/cryptpilot/volumes
/etc/cryptpilot/volumes/otp.toml.template
/etc/cryptpilot/volumes/kbs.toml.template
/etc/cryptpilot/volumes/kms.toml.template
/etc/cryptpilot/volumes/oidc.toml.template
/etc/cryptpilot/volumes/exec.toml.template

%post -n cryptpilot-crypt
# Reload systemd manager configuration to pick up new/updated service files
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || :
fi


%preun -n cryptpilot-crypt
if [ $1 == 0 ]; then #uninstall
  systemctl unmask cryptpilot.service
  systemctl stop cryptpilot.service
  systemctl disable cryptpilot.service
fi


%postun -n cryptpilot-crypt
if [ $1 == 0 ]; then #uninstall
  systemctl daemon-reload
  systemctl reset-failed
fi


%changelog
* Tue Jan 13 2026 Kun Lai <laikun@linux.alibaba.com> - 0.3.0-1
- feat(cryptpilot-crypt): add JSON output and volume filtering to show command
- test: support running integration tests without external binaries
- docs: restructure documentation and add Chinese translations
- refactor: split monolithic package into cryptpilot-fde and cryptpilot-crypt
- refactor(cryptpilot): improve tracing and logging for volume setup and blktrace operations
- build(cryptpilot-verity): split cryptpilot-verity into separate RPM package and improve build/install scripts
- refactor(verity): split verity subcommand into standalone binary cryptpilot-verity
- refactor: change project to rust workspace

* Fri Jan 09 2026 Kun Lai <laikun@linux.alibaba.com> - 0.2.10-1
- fde: Remove the warning print when obtaining the reference value
- fde: support ubuntu GRUB configuration
- fde: improve external disk detection
- dracut: improve cryptpilot initrd module
- cryptpilot-convert: add Debian/Ubuntu system support
- build: add Debian/Ubuntu package building support
- fix(convert): fix boot failure in UKI mode by removing NvVars
- chore(clippy): make cargo clippy happy
- fix(fs): align loop device block size with underlying LUKS2 device
- chore(build): remove support for AnoliOS:8 due to libc dependency on gettid
- refactor: replace cryptsetup CLI calls with libcryptsetup-rs bindings
- chore(build): update Rust toolchain to 1.82.0 and improve build reliability
- feat: add cryptpilot-enhance Secure VM disk image hardening tool
- feat(cryptpilot-convert): skip boot partition creation in UKI mode
- feat: add verity command for data integrity verification
- Make attestation-agent and confidential-data-hub optional Suggests dependencies
- refactor: remove custom aa eventlog entries for config and roothash
- feat(fde): support reference value calculation for UKI boot images
- feat: add --uki-append-cmdline option to customize UKI kernel command line
- refactor(initrd): defer initrd generation and move configs into initrd for UKI support
- feat: add --uki option to generate and boot from Unified Kernel Image
- doc(README.md): upgrade to latest Alinux3 image with updated kernel
- refactor(cmd): add child PID callback hook in command execution

* Tue Nov 11 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.9-1
- fix(fde): fix panic due to wrong default hash algo


* Fri Oct 31 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.8-1
- feat(fde): support multiple hash algorithms in show-reference-value (sha1, sha256, sha384, sm3)
- feat(fde): allow show-reference-value to work on non-encrypted disks
- feat(fs): make TmpMountPoint::mount support read-only mode by default
- fix(nbd): wait 1 second after connecting NBD device to ensure partition detection
- fix: set LC_ALL=C before running external commands for consistent output
- build: switch to git clone for source copy in tarball creation
- build: generate ttrpc protocol files in OUT_DIR and clean up attributes
- refactor: fix wrong URL in .proto file
- docs: update AAEL documentation for new tcg2 log format
- reference value: use IETF 4634 compliant hash algorithm names (e.g., SHA-384)
- fde: use --key-file=- consistently to avoid newline issues in LUKS operations
- fde: include both GRUB kernel cmdline variants in reference values
- fde: add CentOS 7 compatibility for boot measurement
- fde: add DM_UDEV_DISABLE_OTHER_RULES_FLAG to hide intermediate cryptpilot devices
- Remove -E option for `file` command in mkfs.rs
- cryptpilot-convert: improve logging for boot partition creation
- boot_service: remove spurious findmnt failure warning during boot
- boot_service: fix LVM resize failures in initramfs with custom lvm.conf
- fde: disable LVM locking in pvresize and lvextend during early boot

* Fri Sep 26 2025 Kun Lai <laikun@linux.alibaba.com> - 0.2.7-1
- fde: auto-expand system PV and data LV on boot
- boot_service: split stage logic into separate modules
- fde: fix path handling in fde mount setup by using path operations
- boot_service: handle OTP-backed data volumes correctly across reboots
- cryptpilot-convert.sh: lock essential packages after install
- fde: hide intermediate device-mapper devices from udev and udisks
- Revert "cryptpilot: add force override root=/dev/mapper/rootfs to cmdline"
- Revert "cryptpilot-convert: force override the mount source for / in /etc/fstab"
- cryptpilot-convert: rename --clean-freed-space to --wipe-freed-space
- dracut: fix in case initrd-root-device.target are missing on some distros e.g. centos 7
- Rewrite file -E to stdout string matching in mkfs.rs
- Rewrite lvcreate --nolocking in mod.rs
- fde: make GPT device detection resilient to command failure
- fde: improve disk mount handling with better error reporting
- dracut: fix network-manager may not exist in centos7
- cryptpilot-convert: suppress ext4 signature warning by forcing LVM creation
- dracut: remove dependency on /usr/lib/systemd/systemd-makefs
- cryptpilot-convert: Add support for network proxy environment variables
- cryptpilot-convert: Be compatible with different e2fsprogs versions


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
