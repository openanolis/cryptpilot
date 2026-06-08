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

    local a
    for a in /dev/nbd[0-9] /dev/nbd[1-9][0-9]; do
        disk::nbd_available "$a" || continue
        echo "$a"
        return 0
    done
    return 1
}

# Allocate an available NBD device, connect it to a qcow2 image, and register cleanup hook.
# Usage: disk::nbd_connect <qcow2_file> <var_name> [--discard=on] [--detect-zeroes=unmap]
# Sets the global variable named by var_name to the connected NBD device path.
disk::nbd_connect() {
    local image_file=$1
    local var_name=$2
    shift 2

    local nbd_dev
    nbd_dev="$(disk::get_available_nbd)" || proc::fatal "no free NBD device for ${var_name}"

    proc::hook_exit "qemu-nbd -d ${nbd_dev} >/dev/null 2>&1 || true"
    qemu-nbd --connect="${nbd_dev}" "$@" "${image_file}"
    sleep 2

    # Assign to caller's variable
    eval "${var_name}=\"${nbd_dev}\""
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
    echo ""
    echo "Options:"
    echo "  -d, --device <device>                                   Deprecated: operating on devices is no longer supported. Use --in/--out instead."
    echo "      --in <input_file>                                   The input OS image file (vhd or qcow2)."
    echo "      --out <output_file>                                 The output OS image file (vhd or qcow2)."
    echo "  -c, --config-dir <cryptpilot_config_dir>                The directory containing cryptpilot configuration files."
    echo "      --rootfs-passphrase <rootfs_encrypt_passphrase>     The passphrase for rootfs encryption."
    echo "      --rootfs-no-encryption                              Skip rootfs encryption, but keep the rootfs measuring feature enabled."
    echo "      --rootfs-part-num <rootfs_part_num>                 The partition number of the rootfs partition on the original disk. By default the tool will"
    echo "                                                          search for the rootfs partition by label='root' and fail if not found. You can override this"
    echo "                                                          behavior by specifying the partition number."
    echo "      --package <rpm_package>                             Specify an RPM package name or path to the RPM file to install in to the disk before"
    echo "  -b, --boot_part_size <size>                             Instead of using the default partition size(512MB), specify the size of the boot partition"
    echo "                                                          converting. This can be specified multiple times."
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

# Install zram kernel modules if needed (for Ubuntu systems)
install_zram_module_if_needed() {
    local rootfs_mount_point="$1"

    # Check if we're in an Ubuntu-like system by looking for the presence of apt
    if [ -x "${rootfs_mount_point}/usr/bin/apt-get" ] && [ -x "${rootfs_mount_point}/usr/bin/dpkg" ]; then
        log::info "Detecting Ubuntu-like system, attempting to install zram kernel modules"

        # Find the kernel version from the currently installed kernel image
        local kernel_version
        kernel_version=$(chroot "${rootfs_mount_point}" bash -c "dpkg -l | grep -oP 'linux-image-\K[0-9.-]+-generic' | head -n1")

        if [ -z "$kernel_version" ]; then
            log::warn "Could not determine standard Ubuntu kernel version, skipping zram installation"
            return 0
        fi

        log::info "Detected kernel version: $kernel_version"

        # Install the modules package with minimal dependencies
        if ! chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            bash -c "apt-get update && apt-get install -y --no-install-recommends --no-install-suggests linux-modules-extra-$kernel_version"; then
            log::warn "Could not install zram modules, possibly due to disk space constraints or missing package"
            return 1
        fi
    fi
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

    # Try to query the version of cryptpilot-fde-host from the current system
    if command -v rpm >/dev/null 2>&1; then
        cryptpilot_fde_version=$(rpm -q cryptpilot-fde-host --qf '%{VERSION}-%{RELEASE}' 2>/dev/null) || cryptpilot_fde_version=""
    elif command -v dpkg-query >/dev/null 2>&1; then
        cryptpilot_fde_version=$(dpkg-query -W -f='${Version}' cryptpilot-fde-host 2>/dev/null) || cryptpilot_fde_version=""
    fi

    local essential_packages_with_version=()
    local essential_package_names=()

    if [ -n "${cryptpilot_fde_version}" ]; then
        log::info "Detected cryptpilot-fde-host version: ${cryptpilot_fde_version}, will install matching cryptpilot-fde-guest"
        essential_packages_with_version+=("cryptpilot-fde-guest-${cryptpilot_fde_version}")
    else
        log::warn "Failed to detect cryptpilot-fde-host version, installing latest cryptpilot-fde-guest"
        essential_packages_with_version+=("cryptpilot-fde-guest")
    fi
    essential_package_names+=("cryptpilot-fde-guest")

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

    local copied_debs=()
    local user_packages=()
    local packages_to_install=()

    # Step 1: Install user-provided packages first
    for package in "${packages[@]}"; do
        if [[ -f "$package" && "$package" == *.deb ]]; then
            # This is a valid .deb file on the host
            base_name=$(basename "$package")
            cp "$package" "${rootfs_mount_point}/tmp/" # Copy into rootfs /tmp/
            copied_debs+=("/tmp/$base_name")           # Record path inside rootfs
            user_packages+=("/tmp/$base_name")         # Add to installation list
        else
            # Assume this is a regular package name (to be installed via apt)
            user_packages+=("$package")
        fi
    done

    # Install user-provided packages
    if [ ${#user_packages[@]} -gt 0 ]; then
        # First ensure apt indexes are available
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get update || true

        # Pre-install common deb dependencies (dracut, dracut-network are required by cryptpilot-fde-guest)
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get install -y dracut dracut-network lvm2 cryptsetup-bin || true

        # Now install the deb packages
        chroot "${rootfs_mount_point}" bash -c "dpkg --configure -a || true"
        chroot "${rootfs_mount_point}" bash -c "dpkg -i $(printf '%s ' "${user_packages[@]}"| sed 's/ $//')" || true

        # Fix any remaining dependency issues
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get -y -f install || true
    fi

    # Step 2: Build essential packages list
    local cryptpilot_fde_version=""

    # Try to query the version of cryptpilot-fde-host from the current system
    if command -v rpm >/dev/null 2>&1; then
        cryptpilot_fde_version=$(rpm -q cryptpilot-fde-host --qf '%{VERSION}-%{RELEASE}' 2>/dev/null || true)
    elif command -v dpkg-query >/dev/null 2>&1; then
        # Extract version from dpkg-query output, removing epoch if present
        cryptpilot_fde_version=$(dpkg-query -W -f='${Version}' cryptpilot-fde-host 2>/dev/null || true)
    fi

    local essential_packages_with_version=()
    local essential_package_names=()

    if [ -n "${cryptpilot_fde_version}" ]; then
        log::info "Detected cryptpilot-fde-host version: ${cryptpilot_fde_version}, will install matching cryptpilot-fde-guest"
        essential_packages_with_version+=("cryptpilot-fde-guest=${cryptpilot_fde_version}")
    else
        log::warn "Failed to detect cryptpilot-fde-host version, installing latest cryptpilot-fde-guest"
        essential_packages_with_version+=("cryptpilot-fde-guest")
    fi
    essential_package_names+=("cryptpilot-fde-guest")

    # Also include apt-utils for better apt handling
    if ! chroot "${rootfs_mount_point}" dpkg -l apt-utils >/dev/null 2>&1; then
        essential_packages_with_version+=("apt-utils")
        essential_package_names+=("apt-utils")
    fi

    # Check and install missing essential packages
    for i in "${!essential_packages_with_version[@]}"; do
        local pkg_with_version="${essential_packages_with_version[$i]}"
        local pkg_name="${essential_package_names[$i]}"

        # Check if package is already installed in chroot
        if chroot "${rootfs_mount_point}" dpkg -l "$pkg_name" 2>/dev/null | grep -q "^ii"; then
            log::info "Package $pkg_name is already installed, skipping"
        else
            log::info "Package $pkg_name is not installed, will install: $pkg_with_version"
            packages_to_install+=("$pkg_with_version")
        fi
    done

    # Install missing essential packages — skip apt-get if only cryptpilot-fde-guest
    # needs installing (already attempted via dpkg -i above); apt-get would fail for RPM version strings.
    local non_cryptpilot=()
    for pkg in "${packages_to_install[@]}"; do
        case "$pkg" in cryptpilot-fde-guest*) ;; *) non_cryptpilot+=("$pkg") ;; esac
    done
    if [ ${#non_cryptpilot[@]} -gt 0 ]; then
        chroot "${rootfs_mount_point}" /usr/bin/env ${http_proxy:+http_proxy=$http_proxy} \
            ${https_proxy:+https_proxy=$https_proxy} \
            ${ftp_proxy:+ftp_proxy=$ftp_proxy} \
            ${rsync_proxy:+rsync_proxy=$rsync_proxy} \
            ${all_proxy:+all_proxy=$all_proxy} \
            ${no_proxy:+no_proxy=$no_proxy} \
            apt-get -y install "${non_cryptpilot[@]}"
    fi

    # Step 4: Install zram kernel module for Ubuntu (best-effort only)
    install_zram_module_if_needed "${rootfs_mount_point}" || log::warn "zram module installation skipped — may not be available for this kernel"

    # Step 5: Lock version for all essential packages (using base package name)
    chroot "${rootfs_mount_point}" apt-mark hold "${essential_package_names[@]}"

    chroot "${rootfs_mount_point}" apt-get clean

    # Remove the copied .deb files from the chroot after installation
    for deb in "${copied_debs[@]}"; do
        rm -f "${rootfs_mount_point}${deb}"
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
#   $5 - (Optional) Boot partition override — if set, use this instead of boot_part/boot_file_path
#   $6 - (Optional) EFI partition override — if set, use this instead of efi_part
#
setup_chroot_mounts() {
    local rootfs="$1"
    local rootfs_file_or_part="$2"
    local efi_part="$3"
    local boot_file_path="$4"
    local boot_override_part="${5:-}"
    local efi_override_part="${6:-}"

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

    # Mount /boot — use override if provided, otherwise follow existing logic
    local boot_target="$rootfs/boot"
    mkdir -p "$boot_target"
    proc::hook_exit "mountpoint -q '$boot_target' && disk::umount_wait_busy '$boot_target'"

    if [ -n "$boot_override_part" ]; then
        mount "$boot_override_part" "$boot_target"
    elif [ "$boot_part_exist" = "false" ]; then
        if [ -n "$boot_file_path" ]; then
            # /boot is part of root or stored as a file (e.g., in embedded systems)
            mount "$boot_file_path" "$boot_target"
        fi
    else
        # /boot has its own partition
        mount "$boot_part" "$boot_target"
    fi

    # Conditionally mount EFI system partition under /boot/efi
    if [ -n "$efi_override_part" ]; then
        local efi_target="$rootfs/boot/efi"
        mkdir -p "$efi_target"
        proc::hook_exit "mountpoint -q '$efi_target' && disk::umount_wait_busy '$efi_target'"
        mount "$efi_override_part" "$efi_target"
    elif [ "$efi_part_exist" = "true" ] && [ -n "$efi_part" ]; then
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

    # Expand the rootfs partition to utilize available disk space before update
    # This ensures sufficient space for package installations that may trigger scripts
    log::info "Expanding rootfs partition to utilize available disk space before update"

    # Get current rootfs partition size information
    local original_size_in_bytes
    original_size_in_bytes=$(blockdev --getsize64 "${rootfs_orig_part}")
    local original_size_mb=$((original_size_in_bytes / 1024 / 1024))

    log::info "Original rootfs partition size: ${original_size_mb}MB (${original_size_in_bytes} bytes)"

    # Use growpart to expand the partition to maximum available space
    # This is more reliable than manual calculations
    if command -v growpart >/dev/null 2>&1; then
        log::info "Using growpart to expand partition ${rootfs_orig_part_num} on device $device"
        if growpart "$device" "$rootfs_orig_part_num"; then
            # Resize the filesystem to fill the new partition size
            log::info "Resizing filesystem to fill new partition size..."
            e2fsck -f "${rootfs_orig_part}" -p || true  # Run e2fsck first to ensure filesystem integrity
            resize2fs "${rootfs_orig_part}"

            # Verify the new size
            local new_size_in_bytes
            new_size_in_bytes=$(blockdev --getsize64 "${rootfs_orig_part}")
            local new_size_mb=$((new_size_in_bytes / 1024 / 1024))

            log::info "New rootfs partition size: ${new_size_mb}MB (${new_size_in_bytes} bytes)"
            log::info "Rootfs partition and filesystem resized successfully"
        else
            log::warn "growpart failed, proceeding with original partition size"
        fi
    else
        log::warn "growpart command not found, proceeding with original partition size"
    fi

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

    # Ensure grubenv exists with saved_entry set.
    # RHEL-based images ship with a grubenv, but Ubuntu cloud images may not have one.
    # cryptpilot-fde-host show-reference-value requires saved_entry to be present.
    echo "Ensuring grubenv with saved_entry exists..."
    grubenv_path=""
    if [ -f /boot/grub2/grubenv ]; then
        grubenv_path=/boot/grub2/grubenv
    elif [ -f /boot/grub/grubenv ]; then
        grubenv_path=/boot/grub/grubenv
    fi
    if [ -z "$grubenv_path" ]; then
        # Create a fresh grubenv file (1024-byte GRUB environment block)
        echo "Creating grubenv file..."
        mkdir -p /boot/grub
        grubenv_path=/boot/grub/grubenv
        # grub-editenv create generates a valid 1024-byte grubenv file
        if command -v grub-editenv >/dev/null 2>&1; then
            grub-editenv "$grubenv_path" create
        elif command -v grub2-editenv >/dev/null 2>&1; then
            grub2-editenv "$grubenv_path" create
        else
            # Fallback: manually create a 1024-byte block with GRUB env magic
            # GRUB env header: "# GRUB Environment Version 1.0\n" (32 bytes) followed by padding
            { printf '# GRUB Environment Version 1.0\n'; dd if=/dev/zero bs=1 count=992 2>/dev/null; } > "$grubenv_path"
        fi
    fi

    # Determine the kernel entry to set as saved_entry.
    # The value should match the menuentry title/id in grub.cfg.
    # For Ubuntu, the menuentry contains the kernel version (e.g., "Ubuntu, with Linux 5.10.134-csv"),
    # so we extract just the version number without the vmlinuz- prefix.
    if [ -n "$(ls /boot/vmlinuz-* 2>/dev/null | grep -v rescue)" ]; then
        kernel_file=$(ls /boot/vmlinuz-* 2>/dev/null | grep -v rescue | sort -V | tail -1)
        kernel_entry=$(basename "$kernel_file" | sed 's/^vmlinuz-//')
    else
        kernel_file=$(ls /boot/vmlinuz* 2>/dev/null | sort -V | tail -1)
        kernel_entry=$(basename "$kernel_file" | sed 's/^vmlinuz-//')
    fi

    if [ -n "$kernel_entry" ] && [ -f "$grubenv_path" ]; then
        echo "Setting saved_entry to $kernel_entry"
        if command -v grub-editenv >/dev/null 2>&1; then
            grub-editenv "$grubenv_path" set saved_entry="$kernel_entry"
        elif command -v grub2-editenv >/dev/null 2>&1; then
            grub2-editenv "$grubenv_path" set saved_entry="$kernel_entry"
        else
            # Fallback: write directly into grubenv file (text before padding)
            # Remove any existing saved_entry line and add new one
            sed -i '/^saved_entry=/d' "$grubenv_path"
            sed -i "1a saved_entry=$kernel_entry" "$grubenv_path"
        fi
    fi

    # Also ensure GRUB_DEFAULT=saved in /etc/default/grub so GRUB actually uses saved_entry
    if [ -f /etc/default/grub ]; then
        sed -i 's/^GRUB_DEFAULT=.*/GRUB_DEFAULT=saved/' /etc/default/grub
    fi
    for f in /etc/default/grub.d/*.cfg; do
        [ -f "$f" ] && sed -i 's/^GRUB_DEFAULT=.*/GRUB_DEFAULT=saved/' "$f"
    done

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

# shellcheck disable=SC2154
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

    # File mode: mount rootfs from source-write, EFI/boot from output
    local rootfs_mount_point="${workdir}/rootfs"
    local source_write_rootfs_part="${source_write_device}p${source_rootfs_part_num}"

    # Clear the read-only flag set by tune2fs during shrink, so we can mount rw for dracut
    log::info "Clearing read-only flag on source-write rootfs"
    tune2fs -O ^read-only "${source_write_rootfs_part}" >/dev/null 2>&1 || true

    # Determine boot partition for output device (empty in UKI mode — /boot lives in rootfs)
    local boot_override_part=""
    if [ "$uki" = false ] && [ -n "${boot_part_num:-}" ]; then
        boot_override_part="${output_device}p${boot_part_num}"
    fi

    setup_chroot_mounts "${rootfs_mount_point}" "${source_write_rootfs_part}" "${output_device}p${efi_part_num}" "${boot_file_path}" "${boot_override_part}" "${output_device}p${efi_part_num}"

    # Run dracut
    log::info "Executing dracut in chroot"
    update_initrd_inner "${rootfs_mount_point}" "${uki}" "${uki_append_cmdline}"

    cleanup_chroot_mounts "${rootfs_mount_point}"

    sync
}

step::shrink_rootfs() {
    local rootfs_orig_part=$1

    # Mark the rootfs partition as read-only
    tune2fs -O read-only "${rootfs_orig_part}"

    # Adjust file system content, all move to front
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
    after_shrink_size_in_bytes=$((after_shrink_block_size * after_shrink_block_count))
    local after_shrink_size_in_sector
    after_shrink_size_in_sector=$((after_shrink_block_size * after_shrink_block_count / sector_size))
    log::info "Information about the shrinked rootfs:"
    echo "    Block size: $after_shrink_block_size"
    echo "    Block count: $after_shrink_block_count"
    echo "    Size in Bytes: $after_shrink_size_in_bytes"
    echo "    Size in Sector: $after_shrink_size_in_sector"
}

# shellcheck disable=SC2154
step::prepare_output_and_snapshots() {

    # Save the source rootfs partition number before any output modifications.
    # source-read/source-write keep the original partition layout; only output changes.
    source_rootfs_part_num="${rootfs_orig_part_num}"

    # Disconnect source-mod NBD (release lock for snapshot creation)
    log::info "Disconnecting source-mod to create snapshots"
    qemu-nbd -d "${source_mod_device}"
    sleep 3

    # Create two snapshots from the same backing file (source-mod)
    log::info "Creating source-read (read-only) and source-write (writable) snapshots"
    source_read_file="${input_file}.source-read"
    source_write_file="${input_file}.source-write"
    qemu-img create -f qcow2 -b "${source_mod_file}" -F qcow2 "${source_read_file}" >/dev/null
    qemu-img create -f qcow2 -b "${source_mod_file}" -F qcow2 "${source_write_file}" >/dev/null
    proc::hook_exit "rm -f ${source_read_file} ${source_write_file}"

    # Connect snapshots using disk::nbd_connect (allocates + connects + registers hook atomically)
    log::info "Connecting source-read snapshot"
    disk::nbd_connect "${source_read_file}" source_read_device
    log::info "Connecting source-write snapshot"
    disk::nbd_connect "${source_write_file}" source_write_device
    partprobe "${source_read_device}"
    partprobe "${source_write_device}"
    partprobe "${output_device}"
    udevadm settle --timeout=10

    # Copy the partition table from source-read to output
    # (rootfs partition is preserved since we didn't delete it in shrink)
    log::info "Copying partition table to output file"
    sfdisk -d "${source_read_device}" > "${workdir}/partition_table.sfdisk"
    if ! sfdisk "${output_device}" < "${workdir}/partition_table.sfdisk"; then
        log::error "Failed to copy partition table to output"
        return 1
    fi
    partprobe "${output_device}" 2>/dev/null || true
    udevadm settle --timeout=10

    # In UKI mode, resize the EFI partition to accommodate the UKI image (~250MB).
    # The source image may have a small EFI partition that's too small for the UKI.
    if [ "$uki" = true ]; then
        local uki_efi_size="512M"

        # Check if EFI partition is already large enough (>= 250M)
        local efi_size_bytes
        efi_size_bytes=$(blockdev --getsize64 "${output_device}p${efi_part_num}" 2>/dev/null || echo 0)
        local efi_size_mb=$((efi_size_bytes / 1024 / 1024))
        if [[ $efi_size_mb -ge 250 ]]; then
            log::info "UKI mode: EFI partition already large enough (${efi_size_mb}M), skipping resize"
        else
            log::info "UKI mode: resizing EFI partition to ${uki_efi_size} (currently ${efi_size_mb}M)"

            # Save partition UUIDs before modifying them.
            local saved_efi_uuid saved_rootfs_uuid saved_boot_uuid=""
            saved_efi_uuid=$(blkid -s PART_UUID -o value "${output_device}p${efi_part_num}" 2>/dev/null || true)
            saved_rootfs_uuid=$(blkid -s PART_UUID -o value "${output_device}p${rootfs_orig_part_num}" 2>/dev/null || true)
            if [ "$boot_part_exist" = "true" ]; then
                saved_boot_uuid=$(blkid -s PART_UUID -o value "${output_device}p${boot_part_num}" 2>/dev/null || true)
            fi

            # Sync kernel partition table view before modifying
            partprobe "${output_device}" 2>/dev/null || true
            udevadm settle --timeout=10

            # Use sfdisk to modify partition table instead of parted to avoid
            # "overlapping partitions" errors on cloud images with non-sequential
            # partition numbering (e.g., partitions 1, 14, 15, 16).
            local sfdisk_dump="${workdir}/output_before_uki.sfdisk"
            sfdisk -d "${output_device}" > "${sfdisk_dump}"

            # Extract EFI partition info
            local efi_start efi_type efi_uuid
            efi_start=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${efi_part_num}" | sed 's/.*start=\s*\([0-9]*\).*/\1/')
            efi_type=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${efi_part_num}" | sed 's/.*type=\s*\([^ ,]*\).*/\1/')
            efi_uuid=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${efi_part_num}" | sed 's/.*uuid=\s*\([^ ,]*\).*/\1/')

            # Calculate EFI partition end sector for 512M
            local sector_size=512
            local efi_end=$(( (512 * 1024 * 1024) / sector_size + efi_start - 1 ))
            local efi_size_sectors=$((efi_end - efi_start + 1))

            # Get the total disk size in sectors
            local disk_sectors
            disk_sectors=$(blockdev --getsz "${output_device}" 2>/dev/null || echo 0)

            # Get the last usable sector from sfdisk dump (GPT backup header uses some space)
            local last_usable_lba
            last_usable_lba=$(grep "^last-lba:" "${sfdisk_dump}" | awk '{print $2}')
            if [ -z "$last_usable_lba" ]; then
                # Fallback: use disk sectors minus GPT backup (typically 33 sectors)
                last_usable_lba=$((disk_sectors - 34))
            fi

            # Calculate new partition positions
            # Layout will be: EFI -> BOOT (if exists) -> rootfs
            local next_free_sector=$((efi_end + 1))
            next_free_sector=$(disk::align_start_sector "${next_free_sector}")

            # Extract BOOT partition info if it exists
            local boot_start="" boot_size_sectors="" boot_type="" boot_uuid=""
            if [ "$boot_part_exist" = "true" ]; then
                boot_start=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${boot_part_num}" | sed 's/.*start=\s*\([0-9]*\).*/\1/')
                boot_size_sectors=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${boot_part_num}" | sed 's/.*size=\s*\([0-9]*\).*/\1/')
                boot_type=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${boot_part_num}" | sed 's/.*type=\s*\([^ ,]*\).*/\1/')
                boot_uuid=$(sfdisk -d "${output_device}" 2>/dev/null | grep "${output_device}p${boot_part_num}" | sed 's/.*uuid=\s*\([^ ,]*\).*/\1/')
                log::info "Original BOOT partition: start=${boot_start}, size=${boot_size_sectors}"
            fi

            # Calculate new rootfs position (after EFI, or after BOOT if it exists)
            local rootfs_new_start="${next_free_sector}"
            if [ "$boot_part_exist" = "true" ]; then
                # BOOT partition will be placed after EFI
                rootfs_new_start=$((next_free_sector + boot_size_sectors))
                rootfs_new_start=$(disk::align_start_sector "${rootfs_new_start}")
            fi
            local rootfs_size_sectors=$((last_usable_lba - rootfs_new_start + 1))

            log::info "New partition layout: EFI(start=${efi_start}, size=${efi_size_sectors}), rootfs(start=${rootfs_new_start}, size=${rootfs_size_sectors}), last-lba=${last_usable_lba}"
            if [ "$boot_part_exist" = "true" ]; then
                local boot_new_start="${next_free_sector}"
                log::info "BOOT partition will be moved to: start=${boot_new_start}, size=${boot_size_sectors}"
            fi

            # Modify the sfdisk dump
            local dev_escaped
            dev_escaped=$(echo "${output_device}" | sed 's|/|\\/|g')

            # Delete old rootfs partition
            sed -i "\|^${dev_escaped}p${rootfs_orig_part_num}[[:space:]]*:|d" "${sfdisk_dump}"

            # Delete old BOOT partition if it exists (we'll add it back at new position)
            if [ "$boot_part_exist" = "true" ]; then
                sed -i "\|^${dev_escaped}p${boot_part_num}[[:space:]]*:|d" "${sfdisk_dump}"
            fi

            # Modify EFI partition size - replace the entire line with new values
            if [ -n "$efi_uuid" ]; then
                sed -i "s|^${dev_escaped}p${efi_part_num}[[:space:]]*:.*|${output_device}p${efi_part_num} : start=${efi_start}, size=${efi_size_sectors}, type=${efi_type}, uuid=${efi_uuid}|" "${sfdisk_dump}"
            else
                sed -i "s|^${dev_escaped}p${efi_part_num}[[:space:]]*:.*|${output_device}p${efi_part_num} : start=${efi_start}, size=${efi_size_sectors}, type=${efi_type}|" "${sfdisk_dump}"
            fi

            # Add BOOT partition at new position (after EFI, before rootfs)
            if [ "$boot_part_exist" = "true" ]; then
                local boot_line="${output_device}p${boot_part_num} : start=${next_free_sector}, size=${boot_size_sectors}, type=${boot_type}"
                if [ -n "$boot_uuid" ]; then
                    boot_line+=", uuid=${boot_uuid}"
                fi
                echo "$boot_line" >> "${sfdisk_dump}"
            fi

            # Add rootfs partition at the end
            local rootfs_type="0FC63DAF-8483-4772-8E79-3D69D8477DE4"
            echo "${output_device}p${rootfs_orig_part_num} : start=${rootfs_new_start}, size=${rootfs_size_sectors}, type=${rootfs_type}" >> "${sfdisk_dump}"

            log::info "Applying modified partition table with sfdisk"

            # Apply the modified partition table
            local sfdisk_output sfdisk_exit_code
            sfdisk_output=$(sfdisk "${output_device}" < "${sfdisk_dump}" 2>&1) && sfdisk_exit_code=0 || sfdisk_exit_code=$?
            if [ $sfdisk_exit_code -ne 0 ]; then
                log::error "Failed to apply modified partition table (exit code: $sfdisk_exit_code)"
                log::error "sfdisk output: $sfdisk_output"
                # Try to continue anyway, the partition table might still be usable
            fi

            # Restore original partition UUIDs
            if [ -n "$saved_efi_uuid" ]; then
                log::info "Restoring EFI partition UUID: $saved_efi_uuid"
                sgdisk -u "${efi_part_num}:${saved_efi_uuid}" "${output_device}" >/dev/null 2>&1 || true
            fi
            if [ -n "$saved_rootfs_uuid" ]; then
                log::info "Restoring rootfs partition UUID: $saved_rootfs_uuid"
                sgdisk -u "${rootfs_orig_part_num}:${saved_rootfs_uuid}" "${output_device}" >/dev/null 2>&1 || true
            fi
            if [ -n "${saved_boot_uuid:-}" ]; then
                log::info "Restoring BOOT partition UUID: $saved_boot_uuid"
                sgdisk -u "${boot_part_num}:${saved_boot_uuid}" "${output_device}" >/dev/null 2>&1 || true
            fi
        fi
    fi

    # If source had no separate boot partition, we need to create one on output
    # (boot content was extracted to boot.img during step 2)
    if [ "$boot_part_exist" = "false" ] && [ "$uki" = false ]; then
        # Delete the rootfs partition first to free up space for the new boot partition.
        # The data is safe on source-read — we only modify the output partition table.
        log::info "Deleting rootfs partition on output to make room for boot partition"

        # Save rootfs partition UUID before parted deletes it.
        local saved_rootfs_uuid
        saved_rootfs_uuid=$(blkid -s PART_UUID -o value "${output_device}p${rootfs_orig_part_num}" 2>/dev/null || true)

        parted "${output_device}" --script -- rm "${rootfs_orig_part_num}"
        partprobe "${output_device}"
        udevadm settle --timeout=10

        # Create boot partition at original rootfs start with exact BOOT_PART_SIZE
        local boot_start_sector
        boot_start_sector=$(disk::align_start_sector "${rootfs_orig_start_sector}")
        local boot_size_sectors=$(( ${BOOT_PART_SIZE%M} * 1024 * 1024 / sector_size ))
        local boot_end_sector=$((boot_start_sector + boot_size_sectors - 1))
        log::info "Creating boot partition: ${boot_start_sector}s - ${boot_end_sector}s (${BOOT_PART_SIZE})"
        parted "${output_device}" --script -- mkpart boot ext4 "${boot_start_sector}s" "${boot_end_sector}s"

        # Recreate rootfs partition to fill remaining space (from after boot to end of disk)
        local rootfs_new_start=$((boot_end_sector + 1))
        rootfs_new_start=$(disk::align_start_sector "${rootfs_new_start}")
        log::info "Recreating rootfs partition: ${rootfs_new_start}s to end of disk"
        parted "${output_device}" --script -- mkpart primary ext4 "${rootfs_new_start}s" '100%'

        # Re-detect partitions
        partprobe "${output_device}"
        udevadm settle --timeout=10

        # Re-detect partition numbers on output.
        # Since we deleted the old rootfs and created boot + new rootfs,
        # the two highest-numbered partitions are boot and rootfs.
        local all_parts
        all_parts=$(parted "${output_device}" --script -- print 2>/dev/null | awk 'NR>7 && /^[[:space:]]*[0-9]+/ {print $1}')
        local max_part=0
        for p in $all_parts; do
            [[ $p -gt $max_part ]] && max_part=$p
        done
        rootfs_orig_part_num=$max_part
        boot_part_num=$((max_part - 1))

        if [ -z "$boot_part_num" ] || [ -z "$rootfs_orig_part_num" ]; then
            proc::fatal "Failed to detect new partition numbers on output device"
        fi

        # Set boot flag
        parted "${output_device}" --script -- set "${boot_part_num}" boot on

        # Restore original rootfs partition UUID
        if [ -n "$saved_rootfs_uuid" ]; then
            log::info "Restoring rootfs partition UUID: $saved_rootfs_uuid"
            sgdisk -u "${rootfs_orig_part_num}:${saved_rootfs_uuid}" "${output_device}" >/dev/null 2>&1 || true
        fi

        # Format the newly created boot partition with ext4
        log::info "Formatting output boot partition"
        mkfs.ext4 -F "${output_device}p${boot_part_num}" >/dev/null 2>&1
        blockdev --flushbufs "${output_device}"

        # Track output boot partition number separately from the original source detection.
        # boot_part_exist reflects whether the SOURCE had a boot partition.
    else
        partprobe "${output_device}"
        udevadm settle --timeout=10
    fi

    log::info "source-read device: ${source_read_device}"
    log::info "source-write device: ${source_write_device}"
    log::info "output device: ${output_device}"
    lsblk "${output_device}"
}

step::copy_partitions() {

    # dd EFI partition (preserve UUID, labels, all metadata)
    log::info "Copying EFI partition"
    dd if="${source_read_device}p${efi_part_num}" of="${output_device}p${efi_part_num}" bs=4M status=progress

    # Populate the output boot partition.
    # Cases:
    # 1. Source already had a boot partition → dd from source-read (raw copy, preserves filesystem)
    # 2. Source had /boot inside rootfs → copy kernel files from boot.img to the ext4-formatted output partition
    # Note: In UKI mode with boot_part_exist=true, we still need to copy the boot partition
    # because the partition was moved to a new location in step 5.
    if [ "$boot_part_exist" = "true" ]; then
        # Source already had a separate boot partition — raw copy preserves UUID and filesystem
        log::info "Copying boot partition from source"
        dd if="${source_read_device}p${boot_part_num}" of="${output_device}p${boot_part_num}" bs=4M status=progress
    elif [ "$uki" = false ]; then
        # Boot partition was created in step 5, formatted with ext4.
        # Get the boot.img filesystem UUID before copying, then set the
        # output boot partition to the same UUID so that grub.cfg (which
        # references the boot.img UUID) points to the correct partition.
        local boot_img_uuid
        boot_img_uuid=$(blkid -s UUID -o value "${boot_file_path}" 2>/dev/null || true)

        log::info "Copying boot content from boot.img to output boot partition"
        local boot_img_mount="${workdir}/boot_img"
        local boot_part_mount="${workdir}/boot_part"
        mkdir -p "$boot_img_mount" "$boot_part_mount"

        mount -o ro "${boot_file_path}" "$boot_img_mount"
        mount "${output_device}p${boot_part_num}" "$boot_part_mount"
        cp -a "$boot_img_mount/." "$boot_part_mount/"
        disk::umount_wait_busy "$boot_part_mount"
        disk::umount_wait_busy "$boot_img_mount"

        # Set the output boot partition UUID to match boot.img's UUID.
        # tune2fs requires a freshly checked filesystem, so run e2fsck first.
        if [ -n "$boot_img_uuid" ]; then
            log::info "Setting boot partition UUID to $boot_img_uuid"
            e2fsck -f -y "${output_device}p${boot_part_num}" >/dev/null 2>&1 || true
            tune2fs -U "$boot_img_uuid" "${output_device}p${boot_part_num}"
        fi
    fi
}

step::setup_lvm() {
    local output_rootfs_part="${output_device}p${rootfs_orig_part_num}"

    # Set LVM flag on the rootfs partition
    log::info "Setting LVM flag on rootfs partition"
    parted "${output_device}" --script -- set "${rootfs_orig_part_num}" lvm on
    partprobe "${output_device}"
    udevadm settle --timeout=10

    # Initialize LVM physical volume and volume group
    log::info "Initializing LVM physical volume and volume group 'cryptpilot'"
    pvcreate --force "${output_rootfs_part}"
    vgcreate --force cryptpilot "${output_rootfs_part}" --setautoactivation n
}

step::setup_rootfs_lv_with_encrypt() {
    local rootfs_passphrase=$1

    local source_rootfs_part="${source_read_device}p${source_rootfs_part_num}"

    # Calculate filesystem size for LV allocation
    local fs_block_size fs_block_count rootfs_size
    fs_block_size=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block size' | awk '{print $3}')
    fs_block_count=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block count' | awk '{print $3}')
    rootfs_size=$((fs_block_size * fs_block_count))
    # Add 16MB for LUKS2 header overhead
    local rootfs_lv_size=$((rootfs_size + 16 * 1024 * 1024))

    log::info "Creating rootfs logical volume (size: ${rootfs_lv_size} bytes)"
    lvcreate -n rootfs --size "${rootfs_lv_size}"B cryptpilot

    # LUKS on the logical volume
    log::info "Encrypting rootfs logical volume with LUKS2"
    echo -n "${rootfs_passphrase}" | cryptsetup luksFormat \
        --type luks2 --cipher aes-xts-plain64 --subsystem cryptpilot \
        /dev/mapper/cryptpilot-rootfs --key-file=-
    proc::hook_exit "[[ -e /dev/mapper/rootfs ]] && disk::dm_remove_wait_busy rootfs"

    log::info "Opening encrypted rootfs volume"
    echo -n "${rootfs_passphrase}" | cryptsetup open /dev/mapper/cryptpilot-rootfs rootfs --key-file=-

    log::info "Copying rootfs content to the encrypted volume (filesystem: ${rootfs_size} bytes)"
    dd status=progress "if=${source_rootfs_part}" of=/dev/mapper/rootfs bs=4M count="${rootfs_size}" iflag=count_bytes
    disk::dm_remove_wait_busy rootfs
}

step::setup_rootfs_lv_without_encrypt() {
    local source_rootfs_part="${source_read_device}p${source_rootfs_part_num}"

    # Calculate filesystem size for LV allocation
    local fs_block_size fs_block_count rootfs_size
    fs_block_size=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block size' | awk '{print $3}')
    fs_block_count=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block count' | awk '{print $3}')
    rootfs_size=$((fs_block_size * fs_block_count))

    log::info "Creating rootfs logical volume (size: ${rootfs_size} bytes)"
    lvcreate -n rootfs --size "${rootfs_size}"B cryptpilot

    log::info "Copying rootfs content to the logical volume"
    dd status=progress "if=${source_rootfs_part}" of=/dev/mapper/cryptpilot-rootfs bs=4M count="${rootfs_size}" iflag=count_bytes
}

step::setup_rootfs_hash_lv() {
    local source_rootfs_part="${source_read_device}p${source_rootfs_part_num}"
    local rootfs_hash_file_path="${workdir}/rootfs_hash.img"

    # Calculate filesystem size for verity (partition may be larger than actual filesystem)
    local fs_block_size fs_block_count fs_size_bytes data_blocks
    fs_block_size=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block size' | awk '{print $3}')
    fs_block_count=$(dumpe2fs "${source_rootfs_part}" 2>/dev/null | grep 'Block count' | awk '{print $3}')
    fs_size_bytes=$((fs_block_size * fs_block_count))
    data_blocks=$((fs_size_bytes / 4096))  # verity default data block size

    veritysetup format "${source_rootfs_part}" "${rootfs_hash_file_path}" \
        --format=1 --hash=sha256 --data-blocks "${data_blocks}" |
        tee "${workdir}/rootfs_hash.status" |
        gawk '(/^Root hash:/ && $NF ~ /^[0-9a-fA-F]+$/) { print $NF; }' \
            >"${workdir}/rootfs_hash.roothash"
    cat "${workdir}/rootfs_hash.status"

    local rootfs_hash_size_in_byte
    rootfs_hash_size_in_byte=$(stat --printf="%s" "${rootfs_hash_file_path}")

    log::info "Creating rootfs_hash logical volume (size: ${rootfs_hash_size_in_byte} bytes)"
    lvcreate -n rootfs_hash --size "${rootfs_hash_size_in_byte}"B cryptpilot
    dd status=progress "if=${rootfs_hash_file_path}" of=/dev/mapper/cryptpilot-rootfs_hash bs=4M
    rm -f "${rootfs_hash_file_path}"

    log::info "Verity hash stored in logical volume cryptpilot/rootfs_hash"

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
    local source_rootfs_part_num  # Source partition number (unchanged; differs from output when boot partition is added)
    local uki=false
    local uki_append_cmdline="console=tty0 console=ttyS0,115200n8"

    while [[ "$#" -gt 0 ]]; do
        case $1 in
        -d | --device)
            log::warn "--device is deprecated: operating on devices is no longer supported. Use --in/--out instead."
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
            log::warn "--wipe-freed-space is deprecated: no longer needed with the new qcow2 overlay architecture"
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

    if [ -z "${input_file:-}" ] || [ -z "${output_file:-}" ]; then
        proc::fatal "Must specify both --in and --out"
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

    if [ ! -f "$input_file" ]; then
        proc::fatal "Input file $input_file does not exist"
    fi

    # Check if the input file is a vhd or qcow2
    if [[ "$input_file" != *.vhd ]] && [[ "$input_file" != *.qcow2 ]] && [[ "$input_file" != *.img ]]; then
        proc::fatal "Input file $input_file is not supported, should be a vhd, qcow2, or img file"
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
            tool_packages+=(grub-efi-amd64-bin sbsigntool) # Required for UKI (Unified Kernel Image) boot setup
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

    log::info "Using input file: $input_file"
    qemu-img info "${input_file}"

    # Clean up any leftover overlay files from previous runs
    for overlay_file in "${input_file}.source-mod" "${input_file}.source-read" "${input_file}.source-write"; do
        if [ -f "${overlay_file}" ]; then
            log::warn "Temporary file ${overlay_file} already exists from a previous run, deleting it"
            rm -f "${overlay_file}"
        fi
    done

    # Try to detect input file format
    local input_format
    input_format=$(qemu-img info "${input_file}" | grep '^file format:' | awk '{print $3}')

    # Create source-mod overlay: all modifications (yum, grub, shrink) go here
    source_mod_file="${input_file}.source-mod"
    proc::hook_exit "rm -f ${source_mod_file}"
    qemu-img create -f qcow2 -b "${input_file}" -F "${input_format}" "${source_mod_file}" >/dev/null
    log::info "Created source-mod overlay: ${source_mod_file}"

    # Create output file upfront (same virtual size as input)
    local virtual_size_bytes
    # Parse virtual size from text output: "virtual size: 10 GiB (10737418240 bytes)"
    virtual_size_bytes=$(qemu-img info "${input_file}" | grep 'virtual size:' | sed 's/.*(\([0-9]*\) bytes).*/\1/')
    if [[ -z "${virtual_size_bytes}" || ! "${virtual_size_bytes}" =~ ^[0-9]+$ ]]; then
        proc::fatal "Failed to determine virtual size of input image"
    fi
    qemu-img create -f qcow2 -o size="${virtual_size_bytes}" "${output_file}" >/dev/null
    log::info "Created output file: ${output_file} (virtual size: ${virtual_size_bytes} bytes)"

    # Allocate and connect NBD devices one at a time
    disk::nbd_connect "${source_mod_file}" source_mod_device --discard=on --detect-zeroes=unmap
    log::info "Mapped source-mod to NBD device ${source_mod_device}:"
    fdisk -l "${source_mod_device}"

    disk::nbd_connect "${output_file}" output_device --discard=on --detect-zeroes=unmap
    log::info "Mapped output to NBD device ${output_device}:"
    fdisk -l "${output_device}"

    # Alias device to source_mod_device for backward compatibility with partition detection functions
    device="${source_mod_device}"

    # Ensure kernel has partition devices ready for both NBD devices
    partprobe "${source_mod_device}"
    partprobe "${output_device}"
    udevadm settle --timeout=10

    disk::assert_disk_not_busy "${device}"

    # Debug: show what lsblk sees
    log::info "lsblk output for source device:"
    lsblk -lnpo NAME "${source_mod_device}"

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
    # 4. Shrinking rootfs
    #
    log::step "[ 4 ] Shrinking rootfs"
    step::shrink_rootfs "${rootfs_orig_part}"

    #
    # 5. Preparing output file and snapshots
    #
    log::step "[ 5 ] Preparing output file and snapshots"
    step::prepare_output_and_snapshots

    #
    # 6. Copying EFI and boot partitions
    #
    log::step "[ 6 ] Copying EFI and boot partitions"
    step::copy_partitions

    #
    # 7. Setting up rootfs logical volume
    #
    log::step "[ 7 ] Setting up rootfs logical volume"
    step::setup_lvm
    if [ "${rootfs_no_encryption}" = false ]; then
        step::setup_rootfs_lv_with_encrypt "${rootfs_passphrase}"
    else
        step::setup_rootfs_lv_without_encrypt
    fi

    #
    # 8. Setting up rootfs hash volume
    #
    log::step "[ 8 ] Setting up rootfs hash volume"
    step::setup_rootfs_hash_lv

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
            step:update_initrd "${efi_part}" "${boot_file_path}" "${uki}" "${uki_append_cmdline}"
        fi
    fi

    #
    # 10. Cleaning up
    #
    log::step "[ 10 ] Cleaning up"

    #
    # 11. Finalizing
    #
    log::step "[ 11 ] Finalizing"

    # Deactivate LVM volume group before disconnecting NBD
    vgchange -an cryptpilot 2>/dev/null || true
    sleep 1

    # Back up EFI content before NBD disconnect — dracut may have written
    # BOOTX64.EFI (UKI mode) which must be preserved.
    local efi_backup="${workdir}/efi_backup.tar"
    local efi_backup_mount="${workdir}/efi_backup_mnt"
    local efi_label=""
    mkdir -p "$efi_backup_mount"
    if mount "${output_device}p${efi_part_num}" "$efi_backup_mount" 2>/dev/null; then
        # Save the EFI partition label before backing up
        efi_label=$(blkid -s LABEL -o value "${output_device}p${efi_part_num}" 2>/dev/null || true)
        tar cf "$efi_backup" -C "$efi_backup_mount" .
        disk::umount_wait_busy "$efi_backup_mount"
        rmdir "$efi_backup_mount" 2>/dev/null || true
    fi

    # Also back up boot partition if it exists
    local boot_backup="${workdir}/boot_backup.tar"
    local boot_backup_mount="${workdir}/boot_backup_mnt"
    local boot_label=""
    local boot_uuid=""
    if [ -n "${boot_part_num:-}" ]; then
        mkdir -p "$boot_backup_mount"
        if mount "${output_device}p${boot_part_num}" "$boot_backup_mount" 2>/dev/null; then
            # Save the boot partition label and UUID before backing up
            boot_label=$(blkid -s LABEL -o value "${output_device}p${boot_part_num}" 2>/dev/null || true)
            boot_uuid=$(blkid -s UUID -o value "${output_device}p${boot_part_num}" 2>/dev/null || true)
            tar cf "$boot_backup" -C "$boot_backup_mount" .
            disk::umount_wait_busy "$boot_backup_mount"
            rmdir "$boot_backup_mount" 2>/dev/null || true
        fi
    fi

    qemu-nbd -d "${output_device}" 2>/dev/null || true
    qemu-nbd -d "${source_read_device}" 2>/dev/null || true
    qemu-nbd -d "${source_write_device}" 2>/dev/null || true

    # Restore EFI partition using guestfish, which writes directly to qcow2
    # and avoids the NBD writeback-cache data-loss issue on disconnect.
    if [ -f "$efi_backup" ]; then
        local extract_dir="${workdir}/efi_extract"
        mkdir -p "$extract_dir"
        tar xf "$efi_backup" -C "$extract_dir"

        # guestfish uses /dev/sda instead of /dev/nbdX, but partition numbers should match
        # The EFI partition number is determined from the output device partition table
        log::info "Restoring EFI partition (partition ${efi_part_num}) using guestfish"
        if [ -n "$efi_label" ]; then
            log::info "EFI partition label: ${efi_label}"
        fi

        # Build guestfish command with optional label setting
        local guestfish_cmd="run
list-partitions
mkfs vfat /dev/sda${efi_part_num}"

        # Set label if it was saved
        if [ -n "$efi_label" ]; then
            guestfish_cmd+="
set-label /dev/sda${efi_part_num} ${efi_label}"
        fi

        guestfish_cmd+="
mount /dev/sda${efi_part_num} /
copy-in ${extract_dir}/. /
sync"

        if echo "$guestfish_cmd" | guestfish -a "${output_file}"; then
            log::info "EFI partition restored using guestfish"
        else
            log::warn "guestfish failed to restore EFI partition, trying NBD fallback"

            # Fallback: reconnect NBD and restore EFI partition directly
            local fallback_device="/dev/nbd3"
            if qemu-nbd -c "$fallback_device" --format=qcow2 "${output_file}" 2>/dev/null; then
                sleep 2
                partprobe "$fallback_device" 2>/dev/null || true
                udevadm settle --timeout=10

                local fallback_efi_part="${fallback_device}p${efi_part_num}"
                local fallback_mount="${workdir}/efi_fallback_mnt"
                mkdir -p "$fallback_mount"

                # Format and mount the EFI partition
                # Use -n option to set label during filesystem creation
                local mkfs_cmd="mkfs.vfat -F 32"
                if [ -n "$efi_label" ]; then
                    mkfs_cmd+=" -n \"$efi_label\""
                    log::info "Setting EFI partition label: $efi_label"
                fi
                mkfs_cmd+=" \"$fallback_efi_part\""

                if eval "$mkfs_cmd" 2>/dev/null; then
                    if mount "$fallback_efi_part" "$fallback_mount" 2>/dev/null; then
                        # Copy EFI files
                        cp -a "${extract_dir}/." "$fallback_mount/" 2>/dev/null || true
                        sync
                        umount "$fallback_mount" 2>/dev/null || true
                        log::info "EFI partition restored using NBD fallback"
                    else
                        log::warn "Failed to mount EFI partition in fallback"
                    fi
                else
                    log::warn "Failed to format EFI partition in fallback"
                fi

                rmdir "$fallback_mount" 2>/dev/null || true
                qemu-nbd -d "$fallback_device" 2>/dev/null || true
            else
                log::warn "Failed to reconnect NBD for EFI partition fallback"
            fi
        fi

        rm -rf "$extract_dir"
    fi

    # Restore boot partition if it was backed up
    if [ -f "$boot_backup" ] && [ -n "${boot_part_num:-}" ]; then
        local boot_extract_dir="${workdir}/boot_extract"
        mkdir -p "$boot_extract_dir"
        tar xf "$boot_backup" -C "$boot_extract_dir"

        log::info "Restoring boot partition (partition ${boot_part_num}) using NBD"
        if [ -n "$boot_label" ]; then
            log::info "Boot partition label: $boot_label"
        fi

        # Use NBD to restore boot partition (boot is ext4, not vfat)
        local fallback_device="/dev/nbd4"
        if qemu-nbd -c "$fallback_device" --format=qcow2 "${output_file}" 2>/dev/null; then
            sleep 2
            partprobe "$fallback_device" 2>/dev/null || true
            udevadm settle --timeout=10

            local fallback_boot_part="${fallback_device}p${boot_part_num}"
            local fallback_mount="${workdir}/boot_fallback_mnt"
            mkdir -p "$fallback_mount"

            # Format and mount the boot partition (ext4)
            local mkfs_cmd="mkfs.ext4 -F"
            if [ -n "$boot_label" ]; then
                mkfs_cmd+=" -L \"$boot_label\""
            fi
            if [ -n "$boot_uuid" ]; then
                mkfs_cmd+=" -U \"$boot_uuid\""
            fi
            mkfs_cmd+=" \"$fallback_boot_part\""

            if eval "$mkfs_cmd" 2>/dev/null; then
                if mount "$fallback_boot_part" "$fallback_mount" 2>/dev/null; then
                    # Copy boot files
                    cp -a "${boot_extract_dir}/." "$fallback_mount/" 2>/dev/null || true
                    sync
                    umount "$fallback_mount" 2>/dev/null || true
                    log::info "Boot partition restored using NBD fallback"
                else
                    log::warn "Failed to mount boot partition in fallback"
                fi
            else
                log::warn "Failed to format boot partition in fallback"
            fi

            rmdir "$fallback_mount" 2>/dev/null || true
            qemu-nbd -d "$fallback_device" 2>/dev/null || true
        else
            log::warn "Failed to reconnect NBD for boot partition fallback"
        fi

        rm -rf "$boot_extract_dir"
    fi

    log::success "--------------------------------"
    log::success "Everything done, the new disk image is ready to use: ${output_file}"

    echo
    log::info "You can calculate reference value of the disk with:"
    echo ""
    log::highlight "    cryptpilot-fde-host show-reference-value --disk ${output_file}"
}

main "$@"
