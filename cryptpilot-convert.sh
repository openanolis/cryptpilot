#!/bin/bash

set -e # Exit on error
set -u # Exit on undefined variable
shopt -s nullglob

# Ensure consistent locale for parsing.
export LC_ALL=C

# ANSI color codes
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly PURPLE='\033[0;35m'
readonly CYAN='\033[0;36m'
readonly NC='\033[0m' # No Color

# Default boot partition size (can be overridden via --boot_part_size)
BOOT_PART_SIZE="512M"
# Partition alignment in sectors (aligned to 1 MiB boundary)
readonly PARTITION_SECTOR_ALIGNMENT=2048

# Set up logging: redirect stdout/stderr to both terminal and log file,
# and enable shell tracing into the same log.
log::setup_log_file() {
    # https://stackoverflow.com/a/40939603/15011229
    #
    log_file=/tmp/.cryptpilot-convert.log
    exec 3>${log_file}
    # redirect stdout/stderr to a file but also keep them on terminal
    exec 1> >(tee >(cat >&3)) 2>&1

    # https://serverfault.com/a/579078
    #
    # Tell bash to send the trace to log file
    BASH_XTRACEFD=3
    # turn on trace
    set -x
}

# Colored logging functions
log::info() {
    # https://stackoverflow.com/a/7287873/15011229
    #
    # note: printf is used instead of echo to avoid backslash
    # processing and to properly handle values that begin with a '-'.
    printf "${CYAN}ℹ️  %s${NC}\n" "$*" >&2
}

log::success() {
    printf "${GREEN}✅ %s${NC}\n" "$*" >&2
}

log::warn() {
    printf "${YELLOW}⚠️  %s${NC}\n" "$*" >&2
}

log::error() {
    printf "${RED}❌ ERROR: %s${NC}\n" "$*" >&2
}

log::step() {
    printf "${GREEN}▶️  %s${NC}\n" "$*" >&2
}

log::highlight() {
    printf "${PURPLE}📌 %s${NC}\n" "$*" >&2
}

proc::fatal() {
    log::error "$@"
    exit 1
}

# Internal: run before exiting due to error
proc::_trap_cmd_pre() {
    local exit_status=$?
    set +e
    if [[ ${exit_status} -ne 0 ]]; then
        echo
        log::error "Command failed with exit status ${exit_status}. Collecting diagnostic info..."
        (
            echo "===== Diagnostic Info (begin) ====="
            lsblk
            mount
            lsof /dev/nbd* /dev/mapper/* 2>/dev/null || true
            echo "===== Diagnostic Info (end)   ====="
        ) >&3
        log::warn "Full logs saved to: ${log_file}"
    fi
}

# Append a command to one or more traps (e.g., EXIT, INT)
# Usage: proc::_trap_add "command" TRAP_NAME...
proc::_trap_add() {
    local trap_add_cmd=$1
    shift || proc::fatal "${FUNCNAME[0]} usage error"

    # get the num of args
    if [[ $# -eq 0 ]]; then
        proc::fatal "trap name not specitied"
    fi

    for trap_add_name in "$@"; do
        trap -- "$(
            # print the new trap command
            printf 'proc::_trap_cmd_pre\n%s\n' "${trap_add_cmd}"
            # helper fn to get existing trap command from output
            # of trap -p
            # shellcheck disable=SC2329
            proc::_extract_trap_cmd() { printf '%s\n' "${3:-:;}" | sed '/proc::_trap_cmd_pre/d'; }
            # print existing trap command with newline
            eval "proc::_extract_trap_cmd $(trap -p "${trap_add_name}") "
        )" "${trap_add_name}" || proc::fatal "Failed to add command to trap: ${trap_add_name}"
    done
}
declare -f -t proc::_trap_add # Required for modifying DEBUG/RETURN traps

# Register cleanup commands on script exit
# Usage: proc::hook_exit "cleanup_command"
proc::hook_exit() {
    set +x
    if [[ $BASH_SUBSHELL -ne 0 ]]; then
        proc::fatal "proc::hook_exit must not be called from subshell"
    fi
    proc::_trap_add "$1" EXIT INT QUIT TERM
    set -x
}
declare -f -t proc::hook_exit

disk::assert_disk_not_busy() {
    # Check if lvm is using the disk
    if [[ $(lsblk --list -o TYPE "$1" | awk 'NR>1 {print $1}' | grep -c -v -E '(part|disk)') -gt 0 ]]; then
        proc::fatal "The disk is in use, please stop it first."
    fi

    if [[ $(lsblk -l -o MOUNTPOINT "$1" | awk 'NR>1 {print $1}') != "" ]]; then
        proc::fatal "The disk is some where mounted, please unmount it first."
    fi
}

disk::dm_remove_all() {
    local device="$1"
    for dm_name in $(cat <(lsblk "$device" --list | awk 'NR>1 {print $1}') <(dmsetup ls | awk '{print $1}') | sort | uniq -d); do
        disk::dm_remove_wait_busy "$dm_name"
    done
}

disk::align_start_sector() {
    local start_sector=$1
    if ((start_sector % PARTITION_SECTOR_ALIGNMENT != 0)); then
        start_sector=$((((start_sector - 1) / PARTITION_SECTOR_ALIGNMENT + 1) * PARTITION_SECTOR_ALIGNMENT))
    fi
    echo "$start_sector"
}

# https://unix.stackexchange.com/a/312273
disk::nbd_available() {
    [[ $(blockdev --getsize64 "$1") == 0 ]]
}

disk::get_available_nbd() {
    { lsmod | grep nbd >/dev/null; } || modprobe nbd max_part=8
    # If run in container, use following instead
    #
    # mknod /dev/nbd0 b 43 0

    local a
    for a in /dev/nbd[0-9] /dev/nbd[1-9][0-9]; do
        disk::nbd_available "$a" || continue
        echo "$a"
        return 0
    done
    return 1
}

disk::umount_wait_busy() {
    while true; do
        if ! mountpoint -q "$1"; then
            return 0
        fi
        if umount --recursive "$1"; then
            return 0
        fi
        log::warn "Waiting for $1 to be unmounted..."
        sleep 1
    done
}

disk::dm_remove_wait_busy() {
    while true; do
        if ! [ -e /dev/mapper/"$1" ]; then
            return 0
        fi
        if dmsetup remove "$1"; then
            return 0
        fi
        log::warn "Waiting for device mapper $1 to be removed..."
        sleep 1
    done
}

# Print usage help and exit
proc::print_help_and_exit() {
    echo "Usage:"
    echo "    $0 --in <input_file> --out <output_file> --config-dir <cryptpilot_config_dir> --rootfs-passphrase <rootfs_encrypt_passphrase> [--package <rpm_package>...]"
    echo "    $0 --in <input_file> --out <output_file> --config-dir <cryptpilot_config_dir> --rootfs-no-encryption [--package <rpm_package>...]"
    echo "    $0 --device <device> --config-dir <cryptpilot_config_dir> --rootfs-passphrase <rootfs_encrypt_passphrase> [--package <rpm_package>...]"
    echo ""
    echo "Options:"
    echo "  -d, --device <device>                                   The device to operate on."
    echo "      --in <input_file>                                   The input OS image file (vhd or qcow2)."
    echo "      --out <output_file>                                 The output OS image file (vhd or qcow2)."
    echo "  -c, --config-dir <cryptpilot_config_dir>                The directory containing cryptpilot configuration files."
    echo "      --rootfs-passphrase <rootfs_encrypt_passphrase>     The passphrase for rootfs encryption."
    echo "      --rootfs-no-encryption <rootfs_encrypt_passphrase>  Skip rootfs encryption, but keep the rootfs measuring feature enabled."
    echo "      --rootfs-part-num <rootfs_part_num>                 The partition number of the rootfs partition on the original disk. By default the tool will"
    echo "                                                          search for the rootfs partition by label='root' and fail if not found. You can override this"
    echo "                                                          behavior by specifying the partition number."
    echo "      --package <rpm_package>                             Specify an RPM package name or path to the RPM file to install in to the disk before"
    echo "  -b, --boot_part_size <size>                             Instead of using the default partition size(512MB), specify the size of the boot partition"
    echo "                                                          converting. This can be specified multiple times."
    echo "      --wipe-freed-space                                  Wipe the freed space with zero, so that the qemu-img convert would generate smaller image"
    echo "      --uki                                               Generate a Unified Kernel Image image and boot from it instead of boot with GRUB"
    echo "      --uki-append-cmdline <cmdline>                      Append custom command line parameters when generating a UKI image. By default, only essential"
    echo "                                                          parameters are included. This option allows you to extend the kernel command line. The default"
    echo "                                                          value is 'console=tty0 console=ttyS0,115200n8'."
    echo "  -h, --help                                              Show this help message and exit."
    exit "$1"
}

# Execute command in subshell with all file descriptors closed except std*
proc::exec_subshell_flose_fds() {
    (
        set +x
        eval exec {3..255}">&-"
        exec "$@"
    )
}

# Detect rootfs partition by label 'root', or fallback to largest ext*/xfs partition
disk::find_rootfs_partition() {
    local device=$1
    local specified_part_num=$2 # optional specified partition number
    local part_num=1

    if [[ -n "${specified_part_num}" ]]; then
        local part_path=${device}p${specified_part_num}
        if [ ! -b "$part_path" ]; then
            log::error "Specified rootfs partition $part_path does not exist"
            return 1
        fi
        rootfs_orig_part_num="$specified_part_num"
        rootfs_orig_part_exist=true
        return
    fi

    while true; do
        local part_path=${device}p${part_num}
        [ -b "$part_path" ] || break
        local label
        label=$(blkid -o value -s LABEL "$part_path")
        if [ "$label" = "root" ]; then
            rootfs_orig_part_num="$part_num"
            rootfs_orig_part_exist=true
            return
        fi
        part_num=$((part_num + 1))
    done

    # Collect all partition names + sizes, sort by size descending
    mapfile -t parts < <(
        lsblk -lnpo NAME,TYPE,SIZE "$device" |
            awk '$2=="part" {print $1, $3}' |
            sort -k2,2nr
    )

    for entry in "${parts[@]}"; do
        read -r part _ <<<"$entry"

        # Try mounting without specifying fstype
        local rootfs_mount_point=${workdir}/rootfs
        mkdir -p "${rootfs_mount_point}"
        proc::hook_exit "mountpoint -q ${rootfs_mount_point} && disk::umount_wait_busy ${rootfs_mount_point}"
        if mount -o ro "$part" "$rootfs_mount_point" 2>/dev/null; then
            if [[ -d "$rootfs_mount_point/etc" && -d "$rootfs_mount_point/bin" && -d "$rootfs_mount_point/usr" ]]; then
                disk::umount_wait_busy "$rootfs_mount_point"
                rootfs_orig_part_num="${part##*p}"
                rootfs_orig_part_exist=true
                return
            fi
            disk::umount_wait_busy "$rootfs_mount_point"
        fi
    done
}

# find_efi_partition: locate EFI System partition number by multiple heuristics
disk::find_efi_partition() {
    local device=$1

    # Iterate all partition nodes under device
    while IFS= read -r part; do
        [[ "$part" =~ [0-9]+$ ]] || continue

        # 1) PARTLABEL starts with "EFI"
        local label
        label=$(blkid -o value -s PARTLABEL "$part" 2>/dev/null)
        if [[ "$label" == EFI* ]]; then
            efi_part_num="${part##*p}"
            efi_part_exist=true
            return
        fi

        # 2) GPT PARTTYPE GUID matches EFI System GUID
        local ptype
        ptype=$(blkid -o value -s PARTTYPE "$part" 2>/dev/null)
        if [[ "${ptype,,}" == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" ]]; then
            efi_part_num="${part##*p}"
            efi_part_exist=true
            return
        fi

        # 3) vfat filesystem with msdos sec_type
        local sec_type fstype
        sec_type=$(blkid -o value -s SEC_TYPE "$part" 2>/dev/null)
        fstype=$(blkid -o value -s TYPE "$part" 2>/dev/null)
        if [[ "${sec_type,,}" == "msdos" && "${fstype,,}" == "vfat" ]]; then
            efi_part_num="${part##*p}"
            efi_part_exist=true
            return
        fi

        # 4) mount and inspect: must have EFI/ and no vmlinuz-* files
        local efi_mount_point=${workdir}/efi
        mkdir -p "${efi_mount_point}"
        proc::hook_exit "mountpoint -q ${efi_mount_point} && disk::umount_wait_busy ${efi_mount_point}"
        if mount -o ro "$part" "$efi_mount_point" 2>/dev/null; then
            # Check for the existence of the EFI directory
            if [ -d "$efi_mount_point/EFI" ]; then
                # Check that there are no vmlinuz-* files under the root
                vms=("$efi_mount_point"/vmlinuz-*)
                if [ "${#vms[@]}" -eq 0 ]; then
                    disk::umount_wait_busy "$efi_mount_point"
                    efi_part_num="${part##*p}"
                    efi_part_exist=true
                    return
                fi
            fi
            disk::umount_wait_busy "$efi_mount_point"
        fi

    done < <(lsblk -lnpo NAME "$device")
}

disk::find_boot_partition() {
    local device=$1

    while IFS= read -r part; do
        [[ "$part" =~ [0-9]+$ ]] || continue

        local boot_mount_point=${workdir}/boot
        mkdir -p "${boot_mount_point}"
        proc::hook_exit "mountpoint -q ${boot_mount_point} && disk::umount_wait_busy ${boot_mount_point}"
        # Mount Partition (read-only)
        if mount -o ro "$part" "$boot_mount_point" 2>/dev/null; then
            # Check for common boot content directly under mount point
            # Collect all matches
            vms=("$boot_mount_point"/vmlinuz-*)
            if [ "${#vms[@]}" -gt 0 ]; then
                # At least one vmlinuz-* actually exists
                disk::umount_wait_busy "$boot_mount_point"
                boot_part_num="${part##*p}"
                boot_part_exist=true
                return
            fi
            disk::umount_wait_busy "$boot_mount_point"
        fi
    done < <(lsblk -lnpo NAME "$device")
}

step::setup_workdir() {
    # init a tmp workdir with mktemp
    workdir=$(mktemp -d "/tmp/.cryptpilot-convert-XXXXXXXX")
    mkdir -p "${workdir}"
    proc::hook_exit "rm -rf ${workdir}"
}

step::extract_boot_part_from_rootfs() {
    local rootfs_orig_part=$1
    boot_file_path="${workdir}/boot.img"

    log::info "Creating boot partition image of size $BOOT_PART_SIZE"
    fallocate -l "$BOOT_PART_SIZE" "$boot_file_path"
    VERSION=$(mke2fs -V 2>&1 | head -n1 | awk '{print $2}')

    if printf '%s\n' "$VERSION" | grep -qE '^[0-9]+\.[0-9]+(\.[0-9]+)?$'; then
        # Use sort -V for version comparison
        if printf '%s\n' "1.47.0" "$VERSION" | sort -V | head -n1 | grep -q '1.47.0'; then
            echo "e2fsprogs version $VERSION >= 1.47.0, proceeding..."
            yes | mkfs.ext4 -F -O ^orphan_file,^metadata_csum_seed "$boot_file_path"
        else
            echo "e2fsprogs version $VERSION < 1.47.0, skipping advanced features."
            # Fallback to a standard format command
            yes | mkfs.ext4 "$boot_file_path"
        fi
    else
        echo "Could not determine e2fsprogs version."
        exit 1
    fi

    local boot_mount_point=${workdir}/boot
    mkdir -p "$boot_mount_point"
    proc::hook_exit "mountpoint -q ${boot_mount_point} && disk::umount_wait_busy ${boot_mount_point}"
    mount "$boot_file_path" "$boot_mount_point"

    # mount the rootfs
    local rootfs_mount_point=${workdir}/rootfs
    mkdir -p "${rootfs_mount_point}"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point} && disk::umount_wait_busy ${rootfs_mount_point}"
    mount "${rootfs_orig_part}" "${rootfs_mount_point}"

    # extract the /boot content to a boot.img
    log::info "Extracting /boot content to boot partition image"
    cp -a "${rootfs_mount_point}/boot/." "${boot_mount_point}"
    find "${rootfs_mount_point}/boot/" -mindepth 1 -delete

    # When booting alinux3 image with legecy BIOS support in UEFI ECS instance, the real grub.cfg is located at /boot/grub2/grub.cfg, and will be searched by matching path.
    # i.e.:
    # search --no-floppy --set prefix --file /boot/grub2/grub.cfg
    #
    # Here we create a symlink to the boot directory so that grub can find it's grub.cfg.
    ln -s -f . "${boot_mount_point}"/boot

    disk::umount_wait_busy "${boot_mount_point}"
    disk::umount_wait_busy "${rootfs_mount_point}"
}

disk::install_rpm_on_rootfs() {
    local rootfs_mount_point="$1"
    shift
    local packages=("$@")

    local copied_rpms=()  # Will store the local paths inside chroot to the copied .rpm files
    local user_packages=() # User provided packages to install

    # Step 1: Install user-provided packages first
    for package in "${packages[@]}"; do
        if [[ -f "$package" && "$package" == *.rpm ]]; then
            # This is a valid .rpm file on the host
            base_name=$(basename "$package")
            cp "$package" "${rootfs_mount_point}/tmp/" # Copy into rootfs /tmp/
            copied_rpms+=("/tmp/$base_name")           # Record path inside rootfs
            user_packages+=("/tmp/$base_name")         # Add to installation list
        else
            # Assume this is a regular package name (to be installed via yum)
            user_packages+=("$package")
        fi
    done

    # Install user-provided packages
    if [ ${#user_packages[@]} -gt 0 ]; then
        chroot "${rootfs_mount_point}" rpmdb --rebuilddb --dbpath /var/lib/rpm
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            yum install -y "${user_packages[@]}"
    fi

    # Step 2: Build essential packages list
    local cryptpilot_fde_version=""
    
    # Try to query the version of cryptpilot-fde from the current system
    if command -v rpm >/dev/null 2>&1; then
        cryptpilot_fde_version=$(rpm -q cryptpilot-fde --qf '%{VERSION}-%{RELEASE}' 2>/dev/null || true)
    elif command -v dpkg-query >/dev/null 2>&1; then
        cryptpilot_fde_version=$(dpkg-query -W -f='${Version}' cryptpilot-fde 2>/dev/null || true)
    fi
    
    local essential_packages_with_version=()
    local essential_package_names=()
    
    if [ -n "${cryptpilot_fde_version}" ]; then
        log::info "Detected cryptpilot-fde version: ${cryptpilot_fde_version}"
        essential_packages_with_version+=("cryptpilot-fde-${cryptpilot_fde_version}")
    else
        log::warn "Failed to detect cryptpilot-fde version, installing latest version"
        essential_packages_with_version+=("cryptpilot-fde")
    fi
    essential_package_names+=("cryptpilot-fde")
    
    essential_packages_with_version+=("yum-plugin-versionlock")
    essential_package_names+=("yum-plugin-versionlock")

    # Step 3: Check and install missing essential packages
    local packages_to_install=()
    for i in "${!essential_packages_with_version[@]}"; do
        local pkg_with_version="${essential_packages_with_version[$i]}"
        local pkg_name="${essential_package_names[$i]}"
        
        # Check if package is already installed in chroot
        if chroot "${rootfs_mount_point}" rpm -q "$pkg_name" >/dev/null 2>&1; then
            log::info "Package $pkg_name is already installed, skipping"
        else
            log::info "Package $pkg_name is not installed, will install: $pkg_with_version"
            packages_to_install+=("$pkg_with_version")
        fi
    done

    # Install missing essential packages
    if [ ${#packages_to_install[@]} -gt 0 ]; then
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            yum install -y "${packages_to_install[@]}"
    fi

    # Step 4: Lock version for all essential packages (using base package name)
    chroot "${rootfs_mount_point}" yum --cacheonly versionlock "${essential_package_names[@]}"

    chroot "${rootfs_mount_point}" yum clean all

    # Remove the copied .rpm files from the chroot after installation
    for rpm in "${copied_rpms[@]}"; do
        rm -f "${rootfs_mount_point}${rpm}"
    done
}

disk::install_deb_on_rootfs() {
    local rootfs_mount_point="$1"
    shift
    local packages=("$@")

    # Essential packages for Debian/Ubuntu
    local essential_packages=(
        "cryptpilot"
        "attestation-agent"
        "confidential-data-hub"
    )

    local copied_debs=()
    local deb_args=()
    local packages_to_install=()

    for package in "${packages[@]}"; do
        if [[ -f "$package" && "$package" == *.deb ]]; then
            base_name=$(basename "$package")
            cp "$package" "${rootfs_mount_point}/tmp/"
            copied_debs+=("/tmp/$base_name")
            deb_args+=("/tmp/$base_name")
        else
            # For package names, we will ask apt to install after dpkg -i
            deb_args+=("$package")
        fi
    done

    # Check which essential packages are NOT available as local .deb files
    for essential_pkg in "${essential_packages[@]}"; do
        local found=false
        for deb in "${copied_debs[@]}"; do
            if [[ "$deb" == *"${essential_pkg}"* ]]; then
                found=true
                break
            fi
        done
        if [ "$found" = false ]; then
            packages_to_install+=("$essential_pkg")
        fi
    done

    # Try to install .deb files first, then fix dependencies via apt
    if [ ${#deb_args[@]} -gt 0 ]; then
        chroot "${rootfs_mount_point}" bash -c "dpkg --configure -a || true"
        chroot "${rootfs_mount_point}" bash -c "dpkg -i $(printf '%s ' "${deb_args[@]}"| sed 's/ $//')" || true

         # Fix dependencies
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get update || true

        # Fix dependencies
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get -y -f install || true

        # Install only packages not provided as local .deb files
        if [ ${#packages_to_install[@]} -gt 0 ]; then
            chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
                ${https_proxy:+https_proxy=$https_proxy} \
                ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
                ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
                ${all_proxy:+all_proxy=$all_proxy} \
                ${no_proxy:+no_proxy=$no_proxy} \
                apt-get -y install "${packages_to_install[@]}" || true
        fi

        # Hold package versions for essential packages
        for p in "${essential_packages[@]}"; do
            chroot "${rootfs_mount_point}" apt-mark hold "$p" || true
        done
        chroot "${rootfs_mount_point}" apt-get clean || true
    fi

    # cleanup copied debs
    for d in "${copied_debs[@]}"; do
        rm -f "${rootfs_mount_point}${d}"
    done
}

# Sets up a chroot environment by mounting essential filesystems and configurations.
# This includes virtual filesystems (dev, proc, sys, run, pts), boot/efi partitions if applicable,
# and bind-mounts host's resolv.conf and hosts for network access inside chroot.
#
# Note: This function assumes the following global variables are set:
#   - boot_part_exist: "true" if /boot is on a separate partition; "false" otherwise
#   - boot_part: device path of /boot partition (used when boot_part_exist="true")
#   - efi_part_exist: "true" if EFI system partition exists
#
# Arguments:
#   $1 - Root filesystem mount point (e.g., /mnt/rootfs or ${workdir}/rootfs)
#   $2 - Root filesystem device or image file to mount (e.g., /dev/sda2 or ./root.img)
#   $3 - EFI partition device path (optional; e.g., /dev/sda1) — only used if efi_part_exist=true
#   $4 - Boot file/device path (e.g., /dev/sda2 or ./boot.img) — used when boot_part_exist=false
#
setup_chroot_mounts() {
    local rootfs="$1"
    local rootfs_file_or_part="$2"
    local efi_part="$3"
    local boot_file_path="$4"

    log::info "Preparing chroot environment at $rootfs"

    # Ensure the rootfs directory exists
    mkdir -p "$rootfs"

    # Register cleanup hook to safely unmount rootfs on script exit
    proc::hook_exit "mountpoint -q '$rootfs' && disk::umount_wait_busy '$rootfs'"

    # Mount the root filesystem (could be block device or loop-mounted image)
    mount "$rootfs_file_or_part" "$rootfs"

    # Mount required pseudo-filesystems: dev, proc, sys, run, tmp, and devpts
    for dir in dev dev/pts proc run sys tmp; do
        local target="$rootfs/$dir"
        mkdir -p "$target"
        # Register unmount hook for each mount point
        proc::hook_exit "mountpoint -q '$target' && disk::umount_wait_busy '$target'"
        case "$dir" in
        dev) mount -t devtmpfs devtmpfs "$target" ;;
        dev/pts) mount -t devpts devpts "$target" ;;
        proc) mount -t proc proc "$target" ;;
        run) mount -t tmpfs tmpfs "$target" ;;
        sys) mount -t sysfs sysfs "$target" ;;
        tmp) mount -t tmpfs tmpfs "$target" ;;
        esac
    done

    # Mount /boot — either from dedicated partition, from a file/image, or skip in UKI mode if no boot partition
    local boot_target="$rootfs/boot"
    mkdir -p "$boot_target"
    proc::hook_exit "mountpoint -q '$boot_target' && disk::umount_wait_busy '$boot_target'"

    if [ "$boot_part_exist" = "false" ]; then
        if [ -n "$boot_file_path" ]; then
            # /boot is part of root or stored as a file (e.g., in embedded systems)
            mount "$boot_file_path" "$boot_target"
        fi
    else
        # /boot has its own partition
        mount "$boot_part" "$boot_target"
    fi

    # Conditionally mount EFI system partition under /boot/efi
    if [ "$efi_part_exist" = "true" ] && [ -n "$efi_part" ]; then
        local efi_target="$rootfs/boot/efi"
        mkdir -p "$efi_target"
        proc::hook_exit "mountpoint -q '$efi_target' && disk::umount_wait_busy '$efi_target'"
        mount "$efi_part" "$efi_target"
    fi

    # Bind-mount critical network config files from host into chroot (read-only)
    for file in resolv.conf hosts; do
        local src="/etc/$file"
        local dst="$rootfs/etc/$file"
        local backup="$dst.cryptpilot" # Backup original file before bind-mounting

        # Backup existing file in chroot (if any)
        mv "$dst" "$backup" 2>/dev/null || true
        touch "$dst" # Ensure destination exists before bind-mounting

        # Bind-mount host's version as read-only
        proc::hook_exit "mountpoint -q '$dst' && disk::umount_wait_busy '$dst'"
        mount -o bind,ro "$(realpath "$src")" "$dst"
    done
}

# Cleans up all mounted filesystems and restores original configuration files
# after chroot operations are complete. Ensures no mounts remain active.
#
# Arguments:
#   $1 - Root filesystem mount point (same as passed to setup_chroot_mounts)
#
cleanup_chroot_mounts() {
    local rootfs="$1"

    log::info "Cleaning up chroot environment: unmounting all filesystems"

    # Unmount in reverse order (from innermost to outermost)
    for dir in etc/hosts etc/resolv.conf boot/efi boot sys run proc dev/pts dev; do
        disk::umount_wait_busy "$rootfs/$dir" 2>/dev/null || true
    done

    # Restore original resolv.conf and hosts files from backup
    for file in resolv.conf hosts; do
        local dst="$rootfs/etc/$file"
        local backup="$dst.cryptpilot"
        if [ -f "$backup" ]; then
            rm -f "$dst"        # Remove bind-mounted or empty file
            mv "$backup" "$dst" # Restore original content
        fi
    done

    # Finally, unmount the main root filesystem
    disk::umount_wait_busy "$rootfs"
}

# Executes a user-defined function within a fully prepared chroot mount environment.
# Automatically sets up mounts, runs the specified function, then cleans up.
#
# The target function must be defined in the current scope and accept:
#   $1 - Root filesystem mount point
#   $2+ - Any additional arguments passed after the function name
#
# Arguments:
#   $1 - Device or file for root filesystem (e.g., /dev/sda2 or root.img)
#   $2 - EFI partition (e.g., /dev/sda1), optional; pass "" if not used
#   $3 - Boot file/device path (used when boot_part_exist=false)
#   $4 - Name of the function to execute inside the environment
#   $5+ - Optional arguments to pass to the target function
#
# Note:
#   - The root mount point is automatically set to ${workdir}/rootfs
#   - Example usage:
#       run_in_chroot_mounts "/dev/sda2" "/dev/sda1" "./boot.img" "install_grub" "--force"
#
run_in_chroot_mounts() {
    local rootfs_file_or_part="$1" # Root device/file
    local efi_part="$2"            # EFI partition
    local boot_file_path="$3"      # Boot file or device
    local func_name="$4"           # Function to call
    shift 4                        # Shift out first four args; rest go to target function

    local rootfs_mount_point="${workdir}/rootfs"

    # Setup full chroot mount environment
    setup_chroot_mounts "$rootfs_mount_point" "$rootfs_file_or_part" "$efi_part" "$boot_file_path"

    # Execute the provided function with mount point + extra args
    log::info "Executing function '$func_name' inside mounted chroot environment"
    "$func_name" "$rootfs_mount_point" "$@"

    # Clean up all mounts regardless of success/failure
    cleanup_chroot_mounts "$rootfs_mount_point"
}

step:update_rootfs() {
    local efi_part=$1
    local boot_file_path=$2
    local uki=$3

    update_rootfs_inner() {
        local rootfs_mount_point=$1
        local uki=$2

        log::info "Installing packages into target rootfs"
        # Detect package manager inside chroot and choose appropriate installer
        if [ -x "${rootfs_mount_point}/usr/bin/apt-get" ] || [ -x "${rootfs_mount_point}/usr/bin/dpkg" ]; then
            log::info "Detected Debian/Ubuntu rootfs; using DEB installer"
            disk::install_deb_on_rootfs "$rootfs_mount_point" "${packages[@]}"
        else
            log::info "Detected RPM-based rootfs; using RPM installer"
            disk::install_rpm_on_rootfs "$rootfs_mount_point" "${packages[@]}"
        fi

        log::info "Updating /etc/fstab"
        # Prevent duplicate mounting of efi partitions
        sed -i '/[[:space:]]\/boot\/efi[[:space:]]/ s/defaults,/defaults,auto,nofail,/' "${rootfs_mount_point}/etc/fstab"

        if [ "$boot_part_exist" = "false" ] && [ "$uki" = false ]; then
            log::info "Update /etc/fstab for adding /boot mountpoint"
            # update /etc/fstab
            local root_mount_line_number
            root_mount_line_number=$(grep -n -E '^[[:space:]]*[^#][^[:space:]]+[[:space:]]+/[[:space:]]+.*$' "${rootfs_mount_point}/etc/fstab" | head -n 1 | cut -d: -f1)
            if [ -z "${root_mount_line_number}" ]; then
                proc::fatal "Cannot find mount for / in /etc/fstab"
            fi

            ## insert boot mount line
            local boot_uuid
            boot_uuid=$(blkid -o value -s UUID "$boot_file_path") # get uuid of the boot image
            local boot_mount_line="UUID=${boot_uuid} /boot ext4 defaults,auto,nofail 0 2"
            local boot_mount_insert_line_number
            boot_mount_insert_line_number=$((root_mount_line_number + 1))
            sed -i "${boot_mount_insert_line_number}i${boot_mount_line}" "${rootfs_mount_point}/etc/fstab"
        fi

        chroot "${rootfs_mount_point}" bash -c "uki='${uki}' ; $(
            cat <<'EOF'
set -e
set -u

BASH_XTRACEFD=3
set -x
if [ "${uki:-false}" = false ]; then
    # Ensure kernel cmdline includes rd.neednet=1 ip=dhcp for Ubuntu only (prefer cloudimg cfg if present)
    if command -v apt-get >/dev/null 2>&1; then
        GRUB_TARGET="/etc/default/grub.d/50-cloudimg-settings.cfg"
        if [ ! -f "$GRUB_TARGET" ]; then
            GRUB_TARGET="/etc/default/grub"
        fi
        [ -f "$GRUB_TARGET" ] || touch "$GRUB_TARGET"

        grub_add_args() {
            local var="$1"
            local line current
            line=$(grep "^${var}=" "$GRUB_TARGET" | head -n1)
            if [ -n "$line" ]; then
                # Extract value: remove var name and = sign, then remove outer quotes if present
                current="${line#${var}=}"
                current="${current#\"}"
                current="${current%\"}"
            else
                current=""
            fi

            case " $current " in
                *" rd.neednet=1 "*) : ;;
                *) current="$current rd.neednet=1" ;;
            esac
            case " $current " in
                *" ip=dhcp "*) : ;;
                *) current="$current ip=dhcp" ;;
            esac

            # normalize whitespace
            set -- $current
            current="$*"

            local tmp
            tmp=$(mktemp)
            grep -v "^${var}=" "$GRUB_TARGET" > "$tmp" || true
            echo "${var}=\"${current}\"" >> "$tmp"
            mv "$tmp" "$GRUB_TARGET"
        }

        grub_add_args "GRUB_CMDLINE_LINUX_DEFAULT"
    fi

    echo "Updating grub2.cfg"
    grub2_cfg=""
    if [ -e /etc/grub2.cfg ] ; then
        # alinux3 iso with lagecy BIOS support. The real grub2.cfg is in /boot/grub2/grub.cfg
        grub2_cfg=/etc/grub2.cfg
    elif [ -e /etc/grub2-efi.cfg ] ; then
        # alinux3 iso for UEFI only. The real grub2.cfg is in /boot/efi/EFI/alinux/grub.cfg
        grub2_cfg=/etc/grub2-efi.cfg
    elif [ -e /boot/grub2/grub.cfg ] ; then
        # fallback for other distros
        grub2_cfg=/boot/grub2/grub.cfg
    elif [ -e /boot/grub/grub.cfg ] ; then
        grub2_cfg=/boot/grub/grub.cfg
    else
       echo "Cannot find grub config file, will attempt to run update-grub if available"
    fi

    if [ -n "$grub2_cfg" ]; then
        if command -v grub2-mkconfig >/dev/null 2>&1; then
            echo "Generating grub config with grub2-mkconfig -> $grub2_cfg"
            grub2-mkconfig -o "$grub2_cfg" || true
        elif command -v grub-mkconfig >/dev/null 2>&1; then
            echo "Generating grub config with grub-mkconfig -> $grub2_cfg"
            grub-mkconfig -o "$grub2_cfg" || true
        else
            echo "No grub-mkconfig found, will try update-grub if available"
        fi
    else
        if command -v update-grub >/dev/null 2>&1; then
            echo "Running update-grub"
            update-grub || true
        fi
    fi
    echo "Cleaning up package manager cache..."
    if command -v yum >/dev/null 2>&1; then
        yum clean all
        rm -rf /var/lib/dnf/history.*
        rm -rf /var/cache/dnf/*
    fi
    if command -v apt-get >/dev/null 2>&1; then
        apt-get clean
    fi
fi
EOF
        )"

    }

    run_in_chroot_mounts "$rootfs_orig_part" "$efi_part" "$boot_file_path" update_rootfs_inner "${uki}"

}

step:update_initrd() {
    local efi_part=$1
    local boot_file_path=$2
    local uki=$3
    local uki_append_cmdline=$4

    update_initrd_inner() {
        local rootfs_mount_point=$1
        local uki=$2
        local uki_append_cmdline=$3

        # Copy files to the chroot environment
        cp "${workdir}/metadata.toml" "${rootfs_mount_point}/tmp/"
        mkdir -p "${rootfs_mount_point}/tmp/cryptpilot/"
        cp -a "${config_dir}/." "${rootfs_mount_point}/tmp/cryptpilot/"
        # update initrd
        log::info "Updating initrd"
        chroot "${rootfs_mount_point}" bash -c "uki='${uki}' ; uki_append_cmdline='${uki_append_cmdline}' ; $(
            cat <<'EOF'
set -e
set -u

BASH_XTRACEFD=3
set -x

printf '%s\n' 'omit_dracutmodules+=" iscsi nvmf multipath "' > /etc/dracut.conf.d/99-disable-cryptpilot-conflict-modules.conf

KERNELIMG=/boot/vmlinuz
if [ ! -f "$KERNELIMG" ]; then
    echo "No kernel image found at $KERNELIMG, trying fallback..." >&2
    KERNELIMG=$(ls /boot/vmlinuz-* 2>/dev/null | grep -v "rescue" | sort -V | tail -1)
fi

# Parse symbolic links to obtain the real file path
REAL_KERNELIMG=$(readlink -f "$KERNELIMG")

echo "Detected kernel image: $REAL_KERNELIMG"

# Extract the kernel version number
KERNELVER=${REAL_KERNELIMG#/boot/vmlinuz-}
echo "Kernel version: $KERNELVER"

echo "Generating initrd with dracut"

dracut_common_args=(-N --kver "$KERNELVER" --fstab --add-fstab /etc/fstab --force -v)
dracut_common_args+=(--add cryptpilot)
dracut_common_args+=(--include /tmp/metadata.toml /etc/cryptpilot/metadata.toml)
if [[ -f /tmp/cryptpilot/fde.toml ]]; then
    dracut_common_args+=(--include /tmp/cryptpilot/fde.toml /etc/cryptpilot/fde.toml)
fi
if [[ -f /tmp/cryptpilot/global.toml ]]; then
    dracut_common_args+=(--include /tmp/cryptpilot/global.toml /etc/cryptpilot/global.toml)
fi

if [ "${uki:-false}" = true ]; then
    # Remove all existing EFI entries
    find /boot/efi/EFI -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    # Remove NvVars file
    rm -f /boot/efi/NvVars

    # Generate UKI with dracut
    echo "Generating UKI image"
    dracut_args=("${dracut_common_args[@]}" --uefi --hostonly-cmdline)
    cmdline=$(dracut "${dracut_args[@]}" --print-cmdline)
    cmdline=$(echo "${cmdline} ${uki_append_cmdline}" | xargs)
    dracut_args=("${dracut_args[@]}" --kernel-cmdline "$cmdline")

    # Put UKI to /tmp/ instead of /boot directory since objcopy will create a temporary file in the same directory, which may cause no space left error
    TMP_UKI_FILE="/tmp/BOOTX64.EFI"
    FINAL_UKI_FILE="/boot/efi/EFI/BOOT/BOOTX64.EFI"
    dracut "${dracut_args[@]}" "$TMP_UKI_FILE"

    echo "Patching cmdline in UKI"
    # The generated cmdline will have a leading space, remove it
    objcopy --dump-section .cmdline="/tmp/cmdline_full.bin" "$TMP_UKI_FILE"
    cat "/tmp/cmdline_full.bin" | xargs echo -n 2>/dev/null >"/tmp/cmdline_stripped.bin"
    echo -ne "\x00" >>"/tmp/cmdline_stripped.bin"
    objcopy --update-section .cmdline="/tmp/cmdline_stripped.bin" "$TMP_UKI_FILE"
    mkdir -p $(dirname "$FINAL_UKI_FILE")
    cp "$TMP_UKI_FILE" "$FINAL_UKI_FILE"

    echo "UKI successfully created at $FINAL_UKI_FILE, the default boot entry is now overwrited"


else
    echo "Generating new initrd image"
    dracut "${dracut_common_args[@]}"

fi
EOF
        )"

    }

    # Remove read-only flag from rootfs.img
    tune2fs -O ^read-only "${rootfs_file_path}"

    # Note that the rootfs.img will not be used any more so mount it without '-o ro' flag will not change the hash of rootfs.
    run_in_chroot_mounts "$rootfs_file_path" "$efi_part" "$boot_file_path" update_initrd_inner "$uki" "$uki_append_cmdline"
}

step::shrink_and_extract_rootfs_part() {
    local rootfs_orig_part=$1

    # Mark the rootfs partition as read-only
    tune2fs -O read-only "${rootfs_orig_part}"
    
    # Adjust file system content, all move to front
    local before_shrink_size_in_bytes
    before_shrink_size_in_bytes=$(blockdev --getsize64 "${rootfs_orig_part}")
    log::info "Checking and shrinking rootfs filesystem"

    if e2fsck -y -f "${rootfs_orig_part}"; then
        log::info "Filesystem clean or repaired."
    else
        rc=$?
        if [[ $rc -eq 1 ]]; then
            log::info "Filesystem had errors but was fixed."
        else
            log::info "e2fsck failed with exit code $rc"
            return $rc
        fi
    fi

    resize2fs -M "${rootfs_orig_part}"
    # TODO: support filesystem other than ext4
    local after_shrink_block_size
    after_shrink_block_size=$(dumpe2fs "${rootfs_orig_part}" 2>/dev/null | grep 'Block size' | awk '{print $3}')
    local after_shrink_block_count
    after_shrink_block_count=$(dumpe2fs "${rootfs_orig_part}" 2>/dev/null | grep 'Block count' | awk '{print $3}')
    local after_shrink_size_in_bytes
    after_shrink_size_in_bytes=$((after_shrink_block_size * after_shrink_block_count))
    local after_shrink_size_in_sector
    after_shrink_size_in_sector=$((after_shrink_block_size * after_shrink_block_count / sector_size))
    log::info "Information about the shrinked rootfs:"
    echo "    Block size: $after_shrink_block_size"
    echo "    Block count: $after_shrink_block_count"
    echo "    Size in Bytes: $after_shrink_size_in_bytes"
    echo "    Size in Sector: $after_shrink_size_in_sector"

    # Extract rootfs to file on disk
    rootfs_file_path="${workdir}/rootfs.img"
    log::info "Extract rootfs to file on disk ${rootfs_file_path}"
    dd status=progress if="${rootfs_orig_part}" of="${rootfs_file_path}" "count=${after_shrink_size_in_bytes}" iflag=count_bytes bs=256M
    if [ "${wipe_freed_space}" = true ]; then
        log::info "Wipe rootfs partition on device ${before_shrink_size_in_bytes} bytes"
        dd status=progress if=/dev/zero of="${rootfs_orig_part}" count="${before_shrink_size_in_bytes}" iflag=count_bytes bs=64M # Clean the freed space with zero, so that the qemu-img convert would generate smaller image
    fi

    # Delete the original rootfs partition
    log::info "Deleting original rootfs partition"
    parted "$device" --script -- rm "${rootfs_orig_part_num}"
    partprobe "$device" # Inform the OS of partition table changes
}

step::create_boot_part() {
    local boot_file_path=$1
    local boot_start_sector=$2

    local boot_part_num="${rootfs_orig_part_num}"
    local boot_size_in_bytes
    boot_size_in_bytes=$(stat --printf="%s" "$boot_file_path")
    local boot_size_in_sector=$((boot_size_in_bytes / sector_size))
    boot_start_sector=$(disk::align_start_sector "${boot_start_sector}")
    boot_part_end_sector=$((boot_start_sector + boot_size_in_sector - 1))
    log::info "Creating boot partition ($boot_start_sector ... $boot_part_end_sector sectors)"
    parted "$device" --script -- mkpart boot ext4 "${boot_start_sector}"s ${boot_part_end_sector}s
    partprobe "$device"
    udevadm settle --timeout=10
    boot_part="${device}p${boot_part_num}"
    [[ $boot_size_in_bytes == $(blockdev --getsize64 "$boot_part") ]] || log::error "Wrong size, something wrong in the script"
    log::info "Writing boot filesystem to partition"
    dd status=progress if="$boot_file_path" of="$boot_part" bs=4M
}

step::create_lvm_part() {
    local lvm_start_sector=$1
    local lvm_end_sector=$2
    local lvm_part_num=$3

    local lvm_part="${device}p${lvm_part_num}"
    lvm_start_sector=$(disk::align_start_sector "${lvm_start_sector}")
    log::info "Creating lvm partition as LVM PV ($lvm_start_sector ... $lvm_end_sector END sectors)"
    parted "$device" --script -- mkpart primary "${lvm_start_sector}s" "${lvm_end_sector}s"
    parted "$device" --script -- set "${lvm_part_num}" lvm on
    partprobe "$device"

    log::info "Initializing LVM physical volume and volume group"
    proc::exec_subshell_flose_fds pvcreate --force "$lvm_part"
    proc::exec_subshell_flose_fds vgcreate --force system "$lvm_part" --setautoactivation n # disable auto activation of LVM volumes to prevent it from being activated unexpectedly
    proc::exec_subshell_flose_fds vgchange -a y system  # activate the volume group
}

step::setup_rootfs_lv_with_encrypt() {
    local rootfs_file_path=$1
    local rootfs_passphrase=$2

    local rootfs_size_in_byte
    rootfs_size_in_byte=$(stat --printf="%s" "${rootfs_file_path}")
    local rootfs_lv_size_in_bytes=$((rootfs_size_in_byte + 16 * 1024 * 1024)) # original rootfs partition size plus LUKS2 header size
    log::info "Creating rootfs logical volume"
    proc::hook_exit "[[ -e /dev/mapper/system-rootfs ]] && disk::dm_remove_all ${device}"
    proc::exec_subshell_flose_fds lvcreate -n rootfs --size ${rootfs_lv_size_in_bytes}B system # Note that the real size will be a little bit larger than the specified size, since they will be aligned to the Physical Extentsize (PE) size, which by default is 4MB.
    # Create a encrypted volume
    log::info "Encrypting rootfs logical volume with LUKS2"
    echo -n "${rootfs_passphrase}" | cryptsetup luksFormat --type luks2 --cipher aes-xts-plain64 /dev/mapper/system-rootfs --key-file=-
    proc::hook_exit "[[ -e /dev/mapper/rootfs ]] && disk::dm_remove_wait_busy rootfs"

    log::info "Opening encrypted rootfs volume"
    echo -n "${rootfs_passphrase}" | cryptsetup open /dev/mapper/system-rootfs rootfs --key-file=-
    # Copy rootfs content to the encrypted volume
    log::info "Copying rootfs content to the encrypted volume"
    dd status=progress "if=${rootfs_file_path}" of=/dev/mapper/rootfs bs=4M
    disk::dm_remove_wait_busy rootfs
}

step::setup_rootfs_lv_without_encrypt() {
    local rootfs_file_path=$1

    local rootfs_size_in_byte
    rootfs_size_in_byte=$(stat --printf="%s" "${rootfs_file_path}")
    local rootfs_lv_size_in_bytes=$((rootfs_size_in_byte + 16 * 1024 * 1024)) # original rootfs partition size plus LUKS2 header size
    log::info "Creating rootfs logical volume"
    proc::hook_exit "[[ -e /dev/mapper/system-rootfs ]] && disk::dm_remove_all ${device}"
    proc::exec_subshell_flose_fds lvcreate -n rootfs --size ${rootfs_lv_size_in_bytes}B system # Note that the real size will be a little bit larger than the specified size, since they will be aligned to the Physical Extentsize (PE) size, which by default is 4MB.
    # Copy rootfs content to the lvm volume
    log::info "Copying rootfs content to the logical volume"
    dd status=progress "if=${rootfs_file_path}" of=/dev/mapper/system-rootfs bs=4M
}

step::setup_rootfs_hash_lv() {
    local rootfs_file_path=$1
    local rootfs_hash_file_path="${workdir}/rootfs_hash.img"
    veritysetup format "${rootfs_file_path}" "${rootfs_hash_file_path}" --format=1 --hash=sha256 |
        tee "${workdir}/rootfs_hash.status" |
        gawk '(/^Root hash:/ && $NF ~ /^[0-9a-fA-F]+$/) { print $NF; }' \
            >"${workdir}/rootfs_hash.roothash"
    cat "${workdir}/rootfs_hash.status"

    local rootfs_hash_size_in_byte
    rootfs_hash_size_in_byte=$(stat --printf="%s" "${rootfs_hash_file_path}")
    proc::hook_exit "[[ -e /dev/mapper/system-rootfs_hash ]] && disk::dm_remove_all ${device}"
    proc::exec_subshell_flose_fds lvcreate -n rootfs_hash --size "${rootfs_hash_size_in_byte}"B system
    dd status=progress "if=${rootfs_hash_file_path}" of=/dev/mapper/system-rootfs_hash bs=4M
    rm -f "${rootfs_hash_file_path}"
    disk::dm_remove_all "${device}"

    # Recording rootfs hash in metadata file
    log::info "Generate metadata file"
    local roothash
    roothash=$(cat "${workdir}/rootfs_hash.roothash")

    cat <<EOF >"${workdir}/metadata.toml"
type = 1
root_hash = "${roothash}"
EOF

}

main() {
    if [ "$(id -u)" != "0" ]; then
        log::error "This script must be run as root"
        exit 1
    fi

    local operate_on_device
    local device
    local input_file
    local output_file
    local config_dir
    local rootfs_passphrase
    local rootfs_no_encryption=false
    local rootfs_part_num
    local packages=()
    local efi_part_num
    local efi_part_exist=false
    local boot_part
    local boot_part_num
    local boot_part_exist=false
    local rootfs_orig_part
    local rootfs_orig_part_num
    local rootfs_orig_part_exist=false
    local wipe_freed_space=false
    local uki=false
    local uki_append_cmdline="console=tty0 console=ttyS0,115200n8"

    while [[ "$#" -gt 0 ]]; do
        case $1 in
        -d | --device)
            device="$2"
            shift 2
            ;;
        --in)
            input_file="$2"
            shift 2
            ;;
        --out)
            output_file="$2"
            shift 2
            ;;
        -c | --config-dir)
            config_dir="$2"
            shift 2
            ;;
        --rootfs-passphrase)
            rootfs_passphrase="$2"
            shift 2
            ;;
        --rootfs-no-encryption)
            rootfs_no_encryption=true
            shift 1
            ;;
        --rootfs-part-num)
            rootfs_part_num="$2"
            shift 2
            ;;
        --package)
            packages+=("$2")
            shift 2
            ;;
        -b | --boot_part_size)
            BOOT_PART_SIZE=("$2")
            shift 2
            ;;
        --wipe-freed-space)
            wipe_freed_space=true
            shift 1
            ;;
        --uki)
            uki=true
            shift 1
            ;;
        --uki-append-cmdline)
            uki_append_cmdline="$2"
            shift 2
            ;;
        -h | --help)
            proc::print_help_and_exit 0
            ;;
        *)
            proc::fatal "Unexpected argument '$1', please use --help for more information"
            ;;
        esac
    done

    if [ -n "${device:-}" ]; then
        if [ -n "${input_file:-}" ] || [ -n "${output_file:-}" ]; then
            proc::fatal "Cannot specify both --device and --in/--out"
        fi
        operate_on_device=true
    elif [ -n "${input_file:-}" ] && [ -n "${output_file:-}" ]; then
        operate_on_device=false
    else
        proc::fatal "Must specify either --device or --in/--out"
    fi

    if [ -z "${config_dir:-}" ]; then
        proc::fatal "Must specify --config-dir"
    elif [ ! -d "${config_dir}" ]; then
        proc::fatal "Cryptpilot config dir ${config_dir} does not exist"
    else
        [ -f "${config_dir}/fde.toml" ] || proc::fatal "Cryptpilot Full-Disk Encryption config file must exist: ${config_dir}/fde.toml"
    fi

    if [ -n "${rootfs_passphrase:-}" ] && ! [ "${rootfs_no_encryption}" = false ]; then
        proc::fatal "Cannot specify both --rootfs-passphrase and --rootfs-no-encryption"
    elif [ -z "${rootfs_passphrase:-}" ] && [ "${rootfs_no_encryption}" = false ]; then
        proc::fatal "Must specify either --rootfs-passphrase or --rootfs-no-encryption"
    fi

    if [ "${operate_on_device}" = true ]; then
        if [ ! -b "${device}" ]; then
            proc::fatal "Input device $device does not exist"
        fi

        # In a better way to notice user that the data on the device may be lost if the operation is failed or canceled.
        log::warn "This operation will overwrite data on the device ($device), and may cause data loss if the operation is failed or canceled. Make sure you have create a backup of the data !!!"
        while true; do
            read -r -p "Are you sure you want to continue? (y/n) " yn
            case $yn in
            [y]*)
                log::success "Starting to convert the disk ..."
                break
                ;;
            [n]*)
                log::info "Operation canceled."
                exit
                ;;
            *) log::warn "Please answer 'y' or 'n'." ;;
            esac
        done
    elif [ "${operate_on_device}" = false ]; then
        if [ ! -f "$input_file" ]; then
            proc::fatal "Input file $input_file does not exist"
        fi

        # Check if the input file is a vhd or qcow2
        if [[ "$input_file" != *.vhd ]] && [[ "$input_file" != *.qcow2 ]] && [[ "$input_file" != *.img ]]; then
            proc::fatal "Input file $input_file is not supported, should be a vhd or qcow2 file"
        fi
    else
        proc::print_help_and_exit 1
    fi

    log::setup_log_file

    # Install trap to collect error info on exit with error
    proc::hook_exit 'trap "" EXIT' # some shells will call EXIT after the INT handler, so we need to reset the trap at the end
    proc::hook_exit ":;"

    local workdir
    step::setup_workdir

    log::step "[ 0 ] Checking for required tools"

    # Install required host tools via appropriate package manager
    if command -v apt-get >/dev/null 2>&1; then
        local tool_packages=(qemu-utils cryptsetup lvm2 parted grub2-common e2fsprogs lsof fdisk gawk)
        if [[ "$uki" == "true" ]]; then
            tool_packages+=(grub2-tools) # Required for UKI (Unified Kernel Image) boot setup
        fi
        apt-get update
        apt-get install -y "${tool_packages[@]}"
    else
        local tool_packages=(qemu-img cryptsetup veritysetup lvm2 parted e2fsprogs lsof)
        if [[ "$uki" == "true" ]]; then
            tool_packages+=(grub2-tools) # Required for UKI (Unified Kernel Image) boot setup
        fi
        yum install -y "${tool_packages[@]}"
    fi

    #
    # 1. Prepare disk
    #
    log::step "[ 1 ] Prepare disk"

    if [ "$operate_on_device" = true ]; then
        log::info "Using device: $device"
    else
        log::info "Using input file: $input_file"
        qemu-img info "${input_file}"
        device="$(disk::get_available_nbd)" || proc::fatal "no free NBD device"

        local work_file="${input_file}.work"
        if [ -f "${work_file}" ]; then
            if flock --exclusive --nonblock "${work_file}"; then
                log::error "File ${work_file} is locked by another process, maybe another cryptpilot instance is using it. Please stop it and try again."
                exit 1
            else
                log::warn "Temporary file ${work_file} already exists, delete it now"
                rm -f "${work_file}"
            fi
        fi

        # Try to detect input file format
        local input_format
        input_format=$(qemu-img info "${input_file}" | grep '^file format:' | awk '{print $3}')
        
        # Try to create work file with backing file for faster processing
        log::info "Detected input format: ${input_format}"
        proc::hook_exit "rm -f ${work_file}"
        if qemu-img create -f qcow2 -b "${input_file}" -F "${input_format}" "${work_file}" 2>/dev/null; then
            log::info "Created work file ${work_file} with backing file ${input_file}"
        else
            log::warn "Failed to create work file with backing file, falling back to direct copy"
            log::info "Copying ${input_file} to ${work_file}"
            cp "${input_file}" "${work_file}"
        fi

        proc::hook_exit "qemu-nbd --disconnect ${device} >/dev/null"
        qemu-nbd --connect="${device}" --discard=on --detect-zeroes=unmap "${work_file}"
        sleep 2
        log::info "Mapped to NBD device ${device}:"
        fdisk -l "${device}"
    fi

    disk::assert_disk_not_busy "${device}"

    disk::find_efi_partition "${device}"
    [ "${efi_part_exist}" = true ] || proc::fatal "Cannot find EFI partition on $device"
    efi_part="${device}p${efi_part_num}"

    disk::find_boot_partition "${device}"
    if [ "${boot_part_exist}" = true ]; then
        boot_part="${device}p${boot_part_num}"
    else
        log::info "No boot partition on $device"
    fi

    disk::find_rootfs_partition "${device}" "${rootfs_part_num:-}"
    [ "${rootfs_orig_part_exist}" = true ] || proc::fatal "Cannot find rootfs partition on $device"
    rootfs_orig_part="${device}p${rootfs_orig_part_num}"
    rootfs_orig_start_sector=$(parted "$device" --script -- unit s print | grep "^ ${rootfs_orig_part_num}" | awk '{print $2}' | sed 's/s//')
    rootfs_orig_end_sector=$(parted "$device" --script -- unit s print | grep "^ ${rootfs_orig_part_num}" | awk '{print $3}' | sed 's/s//')

    local sector_size
    sector_size=$(blockdev --getss "${device}")
    log::info "Information about the disk:"
    echo "    Device: $device"
    echo "    Sector size: ${sector_size} bytes"
    [ "$efi_part_exist" != "false" ] && echo "    EFI partition: $efi_part"
    [ "$boot_part_exist" != "false" ] && echo "    BOOT partition: $boot_part"
    echo "    Rootfs partition: $rootfs_orig_part"

    #
    # 2. Extracting /boot to boot partition
    #
    log::step "[ 2 ] Extracting /boot to boot partition"
    local boot_file_path=""
    if [ "$boot_part_exist" = "false" ] && [ "$uki" = false ]; then
        step::extract_boot_part_from_rootfs "$rootfs_orig_part"
    elif [ "$boot_part_exist" = "false" ] && [ "$uki" = true ]; then
        log::info "Skipped since UKI mode does not require a separate boot partition"
    else
        log::info "Skipped since boot partition already exist"
    fi

    #
    # 3. Update rootfs
    #
    log::step "[ 3 ] Update rootfs"
    step:update_rootfs "${efi_part}" "${boot_file_path}" "${uki}"

    #
    # 4. Shrinking rootfs and extract
    #
    log::step "[ 4 ] Shrinking rootfs and extract"
    step::shrink_and_extract_rootfs_part "${rootfs_orig_part}"

    #
    # 5. Create a boot partition
    #
    log::step "[ 5 ] Creating boot partition"
    if [ "$boot_part_exist" = "false" ] && [ "$uki" = false ]; then
        local boot_part_end_sector
        step::create_boot_part "${boot_file_path}" "${rootfs_orig_start_sector}"
    elif [ "$boot_part_exist" = "false" ] && [ "$uki" = true ]; then
        log::info "Skipped since UKI mode does not require a separate boot partition"
    else
        log::info "Skipped since boot partition already exist"
    fi

    #
    # 6. Creating lvm partition
    #
    log::step "[ 6 ] Creating lvm partition"
    if [ "$boot_part_exist" = "true" ]; then
        step::create_lvm_part "$rootfs_orig_start_sector" "$rootfs_orig_end_sector" "$rootfs_orig_part_num"
    elif [ "$boot_part_exist" = "false" ] && [ "$uki" = false ]; then
        step::create_lvm_part "$((boot_part_end_sector + 1))" "$rootfs_orig_end_sector" "$((rootfs_orig_part_num + 1))"
    else
        # In UKI mode with no boot partition, we start right after the EFI partition
        # or at the beginning of the available space
        step::create_lvm_part "$rootfs_orig_start_sector" "$rootfs_orig_end_sector" "$rootfs_orig_part_num"
    fi

    #
    # 7. Setting up rootfs logical volume
    #
    log::step "[ 7 ] Setting up rootfs logical volume"
    if [ "${rootfs_no_encryption}" = false ]; then
        step::setup_rootfs_lv_with_encrypt "${rootfs_file_path}" "${rootfs_passphrase}"
    else
        step::setup_rootfs_lv_without_encrypt "${rootfs_file_path}"
    fi

    #
    # 8. Setting up rootfs hash volume
    #
    log::step "[ 8 ] Setting up rootfs hash volume"
    step::setup_rootfs_hash_lv "${rootfs_file_path}"

    #
    # 9. Update initrd
    #
    log::step "[ 9 ] Update initrd"
    if [ "$boot_part_exist" = "true" ]; then
        step:update_initrd "${efi_part}" "${boot_part}" "${uki}" "${uki_append_cmdline}"
    else
        if [ "$uki" = true ]; then
            step:update_initrd "${efi_part}" "" "${uki}" "${uki_append_cmdline}"
        else
            step:update_initrd "${efi_part}" "${boot_part}" "${uki}" "${uki_append_cmdline}"
        fi
    fi

    #
    # 10. Cleaning up
    #
    log::step "[ 10 ] Cleaning up"
    disk::dm_remove_all "${device}"
    blockdev --flushbufs "${device}"

    if [ "${operate_on_device}" == true ]; then
        log::success "--------------------------------"
        log::success "Everything done, the device is ready to use: ${device}"
    else
        #
        # 11. Generating new image file
        #
        log::step "[ 11 ] Generating new image file"
        qemu-nbd --disconnect "${device}"
        sleep 2 # wait for the qemu-nbd daemon to release the file lock

        # check suffix of the output file
        local output_file_suffix=${output_file##*.}
        if [[ "${output_file_suffix}" == "vhd" ]]; then
            qemu-img convert -p -O vpc "${work_file}" "${output_file}"
        elif [[ ${output_file_suffix} == "qcow2" ]]; then
            # It is not worth to enable the compression option "-c", since it does increase the compression time.
            qemu-img convert -p -O qcow2 "${work_file}" "${output_file}"
        else
            log::warn "Unknown output file suffix: ${output_file_suffix}"
            log::info "Generating qcow2 file by default"
            qemu-img convert -p -O qcow2 "${work_file}" "${output_file}"
        fi

        log::success "--------------------------------"
        log::success "Everything done, the new disk image is ready to use: ${output_file}"
    fi

    echo
    log::info "You can calculate reference value of the disk with:"
    echo ""
    log::highlight "    cryptpilot-fde show-reference-value --disk ${output_file}"
}

main "$@"
