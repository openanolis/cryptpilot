#!/bin/bash

set -e # exit on error
set -u # exit when variable not set
shopt -s nullglob

# To avoid locale issues.
export LC_ALL=C
# Color definitions
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly PURPLE='\033[0;35m'
readonly CYAN='\033[0;36m'
readonly NC='\033[0m' # No Color

# the size is currently fixed with 512MB
readonly BOOT_PART_SIZE="512M"
# alignment to 2048 sectors creating a new partition
readonly PARTITION_SECTOR_ALIGNMENT=2048

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
    printf "${CYAN}%s${NC}\n" "$*" >&2
}

log::success() {
    printf "${GREEN}%s${NC}\n" "$*" >&2
}

log::warn() {
    printf "${YELLOW}%s${NC}\n" "$*" >&2
}

log::error() {
    printf "${RED}ERROR: %s${NC}\n" "$*" >&2
}

log::step() {
    printf "${GREEN}%s${NC}\n" "$*" >&2
}

log::highlight() {
    printf "${PURPLE}%s${NC}\n" "$*" >&2
}

proc::fatal() {
    log::error "$@"
    exit 1
}

proc::_trap_cmd_pre() {
    local exit_status=$?
    set +e
    if [[ ${exit_status} -ne 0 ]]; then
        echo
        log::error "Bad exit status ${exit_status}. Collecting error info now ..."
        (
            echo "===== Collecting error info begin ====="
            lsblk
            mount
            lsof
            echo "===== Collecting error info end   ====="
        ) >&3
        log::warn "Please check the error info details in ${log_file}"
    fi
}

# appends a command to a trap
#
# - 1st arg:  code to add
# - remaining args:  names of traps to modify
#
proc::_trap_add() {
    trap_add_cmd=$1
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
        )" "${trap_add_name}" ||
            proc::fatal "unable to add to trap ${trap_add_name}"
    done
}
# set the trace attribute for the above function.  this is
# required to modify DEBUG or RETURN traps because functions don't
# inherit them unless the trace attribute is set
declare -f -t proc::_trap_add

proc::hook_exit() {
    set +x
    if [[ $BASH_SUBSHELL -ne 0 ]]; then
        proc::fatal "proc::hook_exit should not be called from a subshell"
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
    echo "                                                          converting. This can be specified multiple times."
    echo "  -h, --help                                              Show this help message and exit."
    exit "$1"
}

proc::exec_subshell_flose_fds() {
    (
        set +x
        eval exec {3..255}">&-"
        exec "$@"
    )
}

# Determine the right partition number by checking the partition table with partition label
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
    fallocate -l $BOOT_PART_SIZE "$boot_file_path"
    yes | mkfs.ext4 "$boot_file_path"
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

step:update_rootfs_and_initrd() {
    local efi_part=$1
    local boot_file_path=$2

    local rootfs_mount_point=${workdir}/rootfs
    mkdir -p "${rootfs_mount_point}"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point} && disk::umount_wait_busy ${rootfs_mount_point}"
    mount "${rootfs_orig_part}" "${rootfs_mount_point}"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/dev && disk::umount_wait_busy ${rootfs_mount_point}/dev"
    mount -t devtmpfs devtmpfs "${rootfs_mount_point}/dev"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/dev/pts && disk::umount_wait_busy ${rootfs_mount_point}/dev/pts"
    mount -t devpts devpts "${rootfs_mount_point}/dev/pts"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/proc && disk::umount_wait_busy ${rootfs_mount_point}/proc"
    mount -t proc proc "${rootfs_mount_point}/proc"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/run && disk::umount_wait_busy ${rootfs_mount_point}/run"
    mount -t tmpfs tmpfs "${rootfs_mount_point}/run"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/sys && disk::umount_wait_busy ${rootfs_mount_point}/sys"
    mount -t sysfs sysfs "${rootfs_mount_point}/sys"
    # mount bind boot
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/boot && disk::umount_wait_busy ${rootfs_mount_point}/boot"

    if [ "$boot_part_exist" = "false" ]; then
        mount "${boot_file_path}" "${rootfs_mount_point}/boot"
    else
        mount "${boot_part}" "${rootfs_mount_point}/boot"
    fi
    # also mount the EFI part
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/boot/efi && disk::umount_wait_busy ${rootfs_mount_point}/boot/efi"

    if [ "$efi_part_exist" = "true" ]; then
        mount "$efi_part" "${rootfs_mount_point}/boot/efi"
    fi

    log::info "Installing rpm packages"
    packages+=("cryptpilot")
    packages+=("attestation-agent")
    packages+=("confidential-data-hub")

    # shellcheck disable=SC1091
    source "${rootfs_mount_point}"/etc/os-release
    # yum-config-manager --installroot="${rootfs_mount_point}" --add-repo ${YUM_DCAP_REPO}
    if [ ${#packages[@]} -gt 0 ]; then
        if [ "$VERSION" = "23.3" ]; then
            yum --nogpgcheck --releasever="$VERSION" --installroot="${rootfs_mount_point}" install -y "${packages[@]}"
        else
            rpmdb --rebuilddb --dbpath "${rootfs_mount_point}"/var/lib/rpm
            yum --installroot="${rootfs_mount_point}" install -y "${packages[@]}"
        fi
    fi
    yum --installroot="${rootfs_mount_point}" clean all

    # copy cryptpilot config
    log::info "Copying cryptpilot config from ${config_dir} to target rootfs"
    mkdir -p "${rootfs_mount_point}/etc/cryptpilot/"
    cp -a "${config_dir}/." "${rootfs_mount_point}/etc/cryptpilot/"

    # Prevent duplicate mounting of efi partitions
    sed -i '/[[:space:]]\/boot\/efi[[:space:]]/ s/defaults,/defaults,noauto,nofail,/' "${rootfs_mount_point}/etc/fstab"

    if [ "$boot_part_exist" = "false" ]; then
        # update /etc/fstab
        log::info "Updating /etc/fstab"
        local root_mount_line_number
        root_mount_line_number=$(grep -n -E '^[[:space:]]*[^#][^[:space:]]+[[:space:]]+/[[:space:]]+.*$' "${rootfs_mount_point}/etc/fstab" | head -n 1 | cut -d: -f1)
        if [ -z "${root_mount_line_number}" ]; then
            proc::fatal "Cannot find mount for / in /etc/fstab"
        fi

        ## insert boot mount line
        local boot_uuid
        boot_uuid=$(blkid -o value -s UUID "$boot_file_path") # get uuid of the boot image
        local boot_mount_line="UUID=${boot_uuid} /boot ext4 defaults,noauto,nofail 0 2"
        local boot_mount_insert_line_number
        boot_mount_insert_line_number=$((root_mount_line_number + 1))
        sed -i "${boot_mount_insert_line_number}i${boot_mount_line}" "${rootfs_mount_point}/etc/fstab"
    fi

    ## replace the root mount device with /dev/mapper/rootfs
    local root_mount_line_content
    root_mount_line_content=$(grep -E '^[[:space:]]*[^#][^[:space:]]+[[:space:]]+/[[:space:]]+.*$' "${rootfs_mount_point}/etc/fstab" | head -n 1)
    local root_mount_device
    root_mount_device=$(echo "${root_mount_line_content}" | awk '{print $1}')
    sed -i "s|^${root_mount_device}\([[:space:]]\+\)/\([[:space:]]\+\)|/dev/mapper/rootfs\1/\2|" "${rootfs_mount_point}/etc/fstab"

    # Ensure GRUB always uses /dev/mapper/rootfs as root device
    log::info "Configuring GRUB to always use /dev/mapper/rootfs as root device"
    local grub_config_file="${rootfs_mount_point}/etc/default/grub"
    if [ -f "$grub_config_file" ]; then
        # Remove any existing root= parameter from GRUB_CMDLINE_LINUX
        sed -i 's/root=[^[:space:]]*//g' "$grub_config_file"

        # Force to use /dev/mapper/rootfs
        # https://wiki.gentoo.org/wiki/GRUB/Configuration_variables
        echo 'GRUB_DEVICE="/dev/mapper/rootfs"' >>"$grub_config_file"
        echo 'GRUB_DISABLE_LINUX_UUID=true' >>"$grub_config_file"
    else
        log::error "Grub config file ${grub_config_file} not exist"
        return 1
    fi

    # update initrd
    log::info "Updating initrd and grub2"
    chroot "${rootfs_mount_point}" bash -c "$(
        cat <<'EOF'
#!/bin/bash
set -e
set -u

BASH_XTRACEFD=3
set -x

KERNELIMG=$(ls -1 /boot/vmlinuz-* 2>/dev/null | grep -v "rescue" | head -1)
if [ -z "$KERNELIMG" ] ; then
    echo 'No kernel image found, skipping initrd stuff.'>&2
    exit 1
fi
echo "Detected kernel image: $KERNELIMG"
KERNELVER=${KERNELIMG#/boot/vmlinuz-}
echo "Generating initrd with dracut"    
dracut -N --kver $KERNELVER --fstab --add-fstab /etc/fstab --force -v

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
else
    echo "Cannot find grub2 config file"
    exit 1
fi

echo "Detected grub2.cfg, re-generate it now: $grub2_cfg"
grub2-mkconfig -o "$grub2_cfg"

echo "Cleaning up..."
yum clean all
rm -rf /var/lib/dnf/history.*
rm -rf /var/cache/dnf/*

EOF
    )"
    disk::umount_wait_busy "${rootfs_mount_point}/boot/efi"
    disk::umount_wait_busy "${rootfs_mount_point}/boot"
    disk::umount_wait_busy "${rootfs_mount_point}/sys"
    disk::umount_wait_busy "${rootfs_mount_point}/run"
    disk::umount_wait_busy "${rootfs_mount_point}/proc"
    disk::umount_wait_busy "${rootfs_mount_point}/dev/pts"
    disk::umount_wait_busy "${rootfs_mount_point}/dev"

    disk::umount_wait_busy "${rootfs_mount_point}"

}

step::shrink_and_extract_rootfs_part() {
    local rootfs_orig_part=$1

    local before_shrink_size_in_bytes
    before_shrink_size_in_bytes=$(blockdev --getsize64 "${rootfs_orig_part}")

    # Adjust file system content, all move to front
    log::info "Checking and shrinking rootfs filesystem"

    if e2fsck -y -f "${rootfs_orig_part}"; then
        echo "Filesystem clean or repaired."
    else
        rc=$?
        if [[ $rc -eq 1 ]]; then
            echo "Filesystem had errors but was fixed."
        else
            echo "e2fsck failed with exit code $rc"
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
    log::info "Wipe rootfs partition on device ${before_shrink_size_in_bytes} bytes"
    dd status=progress if=/dev/zero of="${rootfs_orig_part}" count="${before_shrink_size_in_bytes}" iflag=count_bytes bs=64M # Clean the freed space with zero, so that the qemu-img convert would generate smaller image

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
    boot_part="${device}p${boot_part_num}"
    [[ $boot_size_in_bytes == $(blockdev --getsize64 "$boot_part") ]] || log::error "Wrong size, something wrong in the script"
    log::info "Writing boot filesystem to partition"
    dd status=progress if="$boot_file_path" of="$boot_part" bs=4M
}

step::create_lvm_part() {
    local lvm_start_sector=$1

    local lvm_end_sector=$2
    if [ "$boot_part_exist" = "true" ]; then
        local lvm_part_num=$rootfs_orig_part_num
    else
        local lvm_part_num=$((rootfs_orig_part_num + 1))
    fi
    local lvm_part="${device}p${lvm_part_num}"
    lvm_start_sector=$(disk::align_start_sector "${lvm_start_sector}")
    echo "Creating lvm partition as LVM PV ($lvm_start_sector ... $lvm_end_sector END sectors)"
    parted "$device" --script -- mkpart primary "${lvm_start_sector}s" "${lvm_end_sector}s"
    parted "$device" --script -- set "${lvm_part_num}" lvm on
    partprobe "$device"

    log::info "Initializing LVM physical volume and volume group"
    proc::exec_subshell_flose_fds pvcreate "$lvm_part"
    proc::exec_subshell_flose_fds vgcreate system "$lvm_part"
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
    echo -n "${rootfs_passphrase}" | cryptsetup luksFormat --type luks2 --cipher aes-xts-plain64 /dev/mapper/system-rootfs -
    proc::hook_exit "[[ -e /dev/mapper/rootfs ]] && disk::dm_remove_wait_busy rootfs"

    log::info "Opening encrypted rootfs volume"
    echo -n "${rootfs_passphrase}" | cryptsetup open /dev/mapper/system-rootfs rootfs -
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
    local boot_part=$2
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

    # Recording rootfs hash in boot partition
    echo "Updating metadata in boot partition"
    local roothash
    roothash=$(cat "${workdir}/rootfs_hash.roothash")
    local boot_mount_point="${workdir}/boot"
    mkdir -p "${boot_mount_point}"
    proc::hook_exit "mountpoint -q ${boot_mount_point} && disk::umount_wait_busy ${boot_mount_point}"
    mount "${boot_part}" "${boot_mount_point}"
    mkdir -p "${boot_mount_point}/cryptpilot/"
    cat <<EOF >"${boot_mount_point}/cryptpilot/metadata.toml"
type = 1
root_hash = "${roothash}"
EOF
    disk::umount_wait_busy "${boot_mount_point}"
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
        if [[ "$input_file" != *.vhd ]] && [[ "$input_file" != *.qcow2 ]]; then
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
    yum install -y qemu-img cryptsetup veritysetup lvm2 parted grub2-tools e2fsprogs lsof

    #
    # 1. Prepare disk
    #
    log::step "[ 1 ] Prepare disk"

    if [ "$operate_on_device" = true ]; then
        log::info "Using device: $device"
    else
        log::info "Using input file: $input_file"
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

        log::info "Copying ${input_file} to ${work_file}"
        proc::hook_exit "rm -f ${work_file}"
        cp "${input_file}" "${work_file}"
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
    local boot_file_path=""
    if [ "$boot_part_exist" = "false" ]; then
        log::step "[ 2 ] Extracting /boot to boot partition"
        step::extract_boot_part_from_rootfs "$rootfs_orig_part"
    fi

    #
    # 3. Update rootfs and initrd
    #
    log::step "[ 3 ] Update rootfs and initrd"
    step:update_rootfs_and_initrd "${efi_part}" "${boot_file_path}"

    #
    # 4. Shrinking rootfs and extract
    #
    log::step "[ 4 ] Shrinking rootfs and extract"
    local boot_file_path
    step::shrink_and_extract_rootfs_part "${rootfs_orig_part}"

    #
    # 5. Create a boot partition
    #
    if [ "$boot_part_exist" = "false" ]; then
        echo "[ 5 ] Creating boot partition"
        local boot_part_end_sector
        step::create_boot_part "${boot_file_path}" "${rootfs_orig_start_sector}"
    fi

    #
    # 6. Creating lvm partition
    #
    log::step "[ 6 ] Creating lvm partition"
    if [ "$boot_part_exist" = "true" ]; then
        step::create_lvm_part "$rootfs_orig_start_sector" "$rootfs_orig_end_sector"
    else
        step::create_lvm_part "$((boot_part_end_sector + 1))" "$rootfs_orig_end_sector"
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
    step::setup_rootfs_hash_lv "${rootfs_file_path}" "${boot_part}"

    #
    # 9. Cleaning up
    #
    log::step "[ 9 ] Cleaning up"
    disk::dm_remove_all "${device}"
    blockdev --flushbufs "${device}"

    if [ "${operate_on_device}" == true ]; then
        log::success "--------------------------------"
        log::success "Everything done, the device is ready to use: ${device}"
    else
        #
        # 10. Generating new image file
        #
        log::step "[ 10 ] Generating new image file"
        qemu-nbd --disconnect "${device}"
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
    log::highlight "    cryptpilot fde show-reference-value --disk ${output_file}"
}

main "$@"
