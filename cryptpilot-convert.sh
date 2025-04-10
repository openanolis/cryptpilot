#!/bin/bash

set -e # exit on error
set -u # exit when variable not set

# To avoid locale issues.
export LC_ALL=C

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

log::info() {
    # https://stackoverflow.com/a/7287873/15011229
    #
    # note: printf is used instead of echo to avoid backslash
    # processing and to properly handle values that begin with a '-'.
    printf '%s\n' "$*"
}

log::error() { log::info "ERROR: $*" >&2; }

proc::fatal() {
    log::error "$@"
    exit 1
}

proc::_trap_cmd_pre() {
    local exit_status=$?
    set +e
    if [[ ${exit_status} -ne 0 ]]; then
        echo
        echo "Bad exit status ${exit_status}. Collecting error info now ..."
        (
            echo "===== Collecting error info begin ====="
            lsblk
            mount
            lsof
            echo "===== Collecting error info end   ====="
        ) >&3
        echo "Please check the error info details in ${log_file}"
    fi
}

# appends a command to a trap
#
# - 1st arg:  code to add
# - remaining args:  names of traps to modify
#
proc::_trap_add() {
    trap_add_cmd=$1
    shift || proc::fatal "${FUNCNAME} usage error"

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
    proc::_trap_add "$1" EXIT INT QUIT TERM
}

declare -f -t proc::hook_exit

disk::assert_disk_not_busy() {
    # Check if lvm is using the disk
    if [[ $(lsblk --list -o TYPE $1 | awk 'NR>1 {print $1}' | grep -v -E '(part|disk)' | wc -l) -gt 0 ]]; then
        proc::fatal "The disk is in use, please stop it first."
    fi

    if [[ $(lsblk -l -o MOUNTPOINT $1 | awk 'NR>1 {print $1}') != "" ]]; then
        proc::fatal "The disk is some where mounted, please unmount it first."
    fi
}

disk::dm_remove_all() {
    local device="$1"
    for dm_name in $(cat <(lsblk "$device" --list | awk 'NR>1 {print $1}') <(dmsetup ls | awk '{print $1}') | sort | uniq -d); do
        dmsetup remove "$dm_name"
    done
}

disk::align_start_sector() {
    local start_sector=$1
    if ((start_sector % PARTITION_SECTOR_ALIGNMENT != 0)); then
        start_sector=$((((start_sector - 1) / PARTITION_SECTOR_ALIGNMENT + 1) * PARTITION_SECTOR_ALIGNMENT))
    fi
    echo $start_sector
}

# https://unix.stackexchange.com/a/312273
disk::nbd_available() {
    [[ $(blockdev --getsize64 $1) -eq 0 ]]
}

disk::get_available_nbd() {
    modprobe nbd max_part=8
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
        if ! mountpoint -q $1; then
            return 0
        fi
        if umount --recursive $1; then
            return 0
        fi
        echo "Waiting for $1 to be unmounted..."
        sleep 1
    done
}

proc::print_help_and_exit() {
    echo "Usage:"
    echo "    $0 --device <device> --config-dir <cryptpilot_config_dir> --passphrase <rootfs_encrypt_passphrase> [--package <rpm_package>...]"
    echo "    $0 --in <input_file> --out <output_file> --config-dir <cryptpilot_config_dir> --passphrase <rootfs_encrypt_passphrase> [--package <rpm_package>...]"
    echo ""
    echo "Options:"
    echo "  -d, --device <device>                           The device to operate on."
    echo "      --in <input_file>                           The input OS image file (vhd or qcow2)."
    echo "      --out <output_file>                         The output OS image file (vhd or qcow2)."
    echo "  -c, --config-dir <cryptpilot_config_dir>        The directory containing cryptpilot configuration files."
    echo "      --passphrase <rootfs_encrypt_passphrase>    The passphrase for rootfs encryption."
    echo "      --package <rpm_package>                     Specify an RPM package name or RPM file to install (can be specified multiple times)."
    echo "  -h, --help                                      Show this help message and exit."
    exit $1
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
    local part_num=1
    while true; do
        local part_path=${device}p${part_num}
        if [ ! -b "$part_path" ]; then
            echo "Cannot find rootfs partition" >&2
            return 1
        fi
        local label
        label=$(blkid -o value -s LABEL $part_path)
        if [ "$label" = "root" ]; then
            # make sure it's the last partition
            local next_part_path=${device}p$((part_num + 1))
            if [ -b "$next_part_path" ]; then
                echo "The rootfs partition should be the last partition" >&2
                return 1
            fi
            echo "$part_num"
            return 0
        fi
        part_num=$((part_num + 1))
    done
}

# find efi partition by PARTLABEL
disk::find_efi_partition() {
    local device=$1
    local part_num=1
    while true; do
        local part_path=${device}p${part_num}
        if [ ! -b "$part_path" ]; then
            break # try again with another method
        fi
        local part_label
        part_label=$(blkid -o value -s PARTLABEL $part_path)
        if [[ "$part_label" == EFI* ]]; then
            echo "$part_num"
            return 0
        fi
        part_num=$((part_num + 1))
    done

    # find efi partition by SEC_TYPE="msdos" and TYPE="vfat"
    local part_num=1
    while true; do
        local part_path=${device}p${part_num}
        if [ ! -b "$part_path" ]; then
            echo "Cannot find efi partition" >&2
            return 1
        fi
        local sec_type
        sec_type=$(blkid -o value -s SEC_TYPE $part_path)
        local part_type
        part_type=$(blkid -o value -s TYPE $part_path)
        if [[ "$sec_type" == "msdos" ]] && [[ "$part_type" == "vfat" ]]; then
            echo "$part_num"
            return 0
        fi
        part_num=$((part_num + 1))
    done
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

    fallocate -l $BOOT_PART_SIZE $boot_file_path
    yes | mkfs.ext4 $boot_file_path
    local boot_mount_point=${workdir}/boot
    mkdir -p $boot_mount_point
    proc::hook_exit "mountpoint -q ${boot_mount_point} && disk::umount_wait_busy ${boot_mount_point}"
    mount $boot_file_path $boot_mount_point

    # mount the rootfs
    local rootfs_mount_point=${workdir}/rootfs
    mkdir -p "${rootfs_mount_point}"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point} && disk::umount_wait_busy ${rootfs_mount_point}"
    mount "${rootfs_orig_part}" "${rootfs_mount_point}"

    # extract the /boot content to a boot.img
    cp -a "${rootfs_mount_point}/boot/." "${boot_mount_point}"
    find "${rootfs_mount_point}/boot/" -mindepth 1 -delete

    # When booting alinux3 image with legecy BIOS support in UEFI ECS instance, the real grub.cfg is located at /boot/grub2/grub.cfg, and will be searched by matching path.
    # i.e.:
    # search --no-floppy --set prefix --file /boot/grub2/grub.cfg
    #
    # Here we create a symlink to the boot directory so that grub can find it's grub.cfg.
    ln -s -f . ${boot_mount_point}/boot

    disk::umount_wait_busy ${boot_mount_point}
}

step:update_rootfs_and_initrd() {
    local efi_part=$1
    local boot_file_path=$2

    local rootfs_mount_point=${workdir}/rootfs
    mkdir -p "${rootfs_mount_point}"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/dev && umount ${rootfs_mount_point}/dev"
    mount -t devtmpfs devtmpfs "${rootfs_mount_point}/dev"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/dev/pts && umount ${rootfs_mount_point}/dev/pts"
    mount -t devpts devpts "${rootfs_mount_point}/dev/pts"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/proc && umount ${rootfs_mount_point}/proc"
    mount -t proc proc "${rootfs_mount_point}/proc"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/run && umount ${rootfs_mount_point}/run"
    mount -t tmpfs tmpfs "${rootfs_mount_point}/run"
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/sys && umount ${rootfs_mount_point}/sys"
    mount -t sysfs sysfs "${rootfs_mount_point}/sys"
    # mount bind boot
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/boot && umount ${rootfs_mount_point}/boot"
    mount "${boot_file_path}" "${rootfs_mount_point}/boot"
    # also mount the EFI part
    proc::hook_exit "mountpoint -q ${rootfs_mount_point}/boot/efi && umount ${rootfs_mount_point}/boot/efi"
    mount "$efi_part" "${rootfs_mount_point}/boot/efi"

    echo "Installing rpm packages"
    packages+=("cryptpilot")
    packages+=("attestation-agent")
    packages+=("confidential-data-hub")
    # yum-config-manager --installroot="${rootfs_mount_point}" --add-repo ${YUM_DCAP_REPO}
    if [ ${#packages[@]} -gt 0 ]; then
        yum --installroot="${rootfs_mount_point}" install -y "${packages[@]}"
    fi
    yum --installroot="${rootfs_mount_point}" clean all

    # copy cryptpilot config
    echo "Copying cryptpilot config from /etc/cryptpilot to target rootfs"
    mkdir -p "${rootfs_mount_point}/etc/cryptpilot/"
    cp -a "${config_dir}." "${rootfs_mount_point}/etc/cryptpilot/"

    # update /etc/fstab
    echo "Updating /etc/fstab"
    local boot_uuid
    boot_uuid=$(blkid -o value -s UUID $boot_file_path) # get uuid of the boot image
    local boot_mount_line="UUID=${boot_uuid} /boot ext4 defaults 0 2"
    local root_mount_line_number
    root_mount_line_number=$(grep -n -E '^[[:space:]]*[^#][^[:space:]]+[[:space:]]+/[[:space:]]+.*$' "${rootfs_mount_point}/etc/fstab" | head -n 1 | cut -d: -f1)
    if [ -z "${root_mount_line_number}" ]; then
        proc::fatal "Cannot find mount for / in /etc/fstab"
    fi
    local boot_mount_insert_line_number
    boot_mount_insert_line_number=$((root_mount_line_number + 1))
    sed -i "${boot_mount_insert_line_number}i${boot_mount_line}" "${rootfs_mount_point}/etc/fstab"

    # update initrd
    echo "Updating initrd and grub2"
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
    umount "${rootfs_mount_point}/boot/efi"
    umount "${rootfs_mount_point}/boot"
    umount "${rootfs_mount_point}/sys"
    umount "${rootfs_mount_point}/run"
    umount "${rootfs_mount_point}/proc"
    umount "${rootfs_mount_point}/dev/pts"
    umount "${rootfs_mount_point}/dev"

    disk::umount_wait_busy "${rootfs_mount_point}"

}

step::shrink_and_extract_rootfs_part() {
    local rootfs_orig_part=$1

    local before_shrink_size_in_bytes
    before_shrink_size_in_bytes=$(blockdev --getsize64 "${rootfs_orig_part}")

    # Adjust file system content, all move to front
    e2fsck -y -f "${rootfs_orig_part}"
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
    echo "Information about the shrinked rootfs:"
    echo "    Block size: $after_shrink_block_size"
    echo "    Block count: $after_shrink_block_count"
    echo "    Size in Bytes: $after_shrink_size_in_bytes"
    echo "    Size in Sector: $after_shrink_size_in_sector"

    # Extract rootfs to file on disk
    rootfs_file_path="${workdir}/rootfs.img"
    echo "Extract rootfs to file on disk ${rootfs_file_path}"
    dd status=progress if="${rootfs_orig_part}" of="${rootfs_file_path}" "count=${after_shrink_size_in_bytes}" iflag=count_bytes bs=256M
    echo "Wipe rootfs partition on device ${before_shrink_size_in_bytes} bytes"
    dd status=progress if=/dev/zero of="${rootfs_orig_part}" count="${before_shrink_size_in_bytes}" iflag=count_bytes bs=64M # Clean the freed space with zero, so that the qemu-img convert would generate smaller image

    # Delete the original rootfs partition
    parted $device --script -- rm "${rootfs_orig_part_num}"
    partprobe $device # Inform the OS of partition table changes
}

step::create_boot_part() {
    local boot_file_path=$1
    local boot_start_sector=$2

    local boot_part_num="${rootfs_orig_part_num}"
    local boot_size_in_bytes
    boot_size_in_bytes=$(stat --printf="%s" $boot_file_path)
    local boot_size_in_sector=$((boot_size_in_bytes / sector_size))
    boot_start_sector=$(disk::align_start_sector ${boot_start_sector})
    boot_part_end_sector=$((boot_start_sector + boot_size_in_sector - 1))
    echo "Creating boot partition ($boot_start_sector ... $boot_part_end_sector sectors)"
    parted $device --script -- mkpart boot ext4 ${boot_start_sector}s ${boot_part_end_sector}s
    boot_part="${device}p${boot_part_num}"
    [[ $boot_size_in_bytes == $(blockdev --getsize64 $boot_part) ]] || echo "Wrong size, something wrong in the script"
    dd status=progress if=$boot_file_path of=$boot_part bs=4M
}

step::create_lvm_part() {
    local lvm_start_sector=$1

    local lvm_part_num=$((rootfs_orig_part_num + 1))
    local lvm_part="${device}p${lvm_part_num}"
    lvm_start_sector=$(disk::align_start_sector "${lvm_start_sector}")
    echo "Creating lvm partition as LVM PV ($lvm_start_sector ... END sectors)"
    parted $device --script -- mkpart system "${lvm_start_sector}s" 100%
    parted $device --script -- set "${lvm_part_num}" lvm on
    partprobe $device

    proc::exec_subshell_flose_fds pvcreate $lvm_part
    proc::exec_subshell_flose_fds vgcreate system $lvm_part
}

step::setup_rootfs_lv() {
    local passphrase=$1
    local rootfs_file_path=$2

    local rootfs_size_in_byte
    rootfs_size_in_byte=$(stat --printf="%s" "${rootfs_file_path}")
    local rootfs_lv_size_in_bytes=$((rootfs_size_in_byte + 16 * 1024 * 1024)) # original rootfs partition size plus LUKS2 header size
    echo "Creating rootfs logical volume"
    proc::hook_exit "[[ -e /dev/mapper/system-rootfs ]] && disk::dm_remove_all ${device}"
    proc::exec_subshell_flose_fds lvcreate -n rootfs --size ${rootfs_lv_size_in_bytes}B system # Note that the real size will be a little bit larger than the specified size, since they will be aligned to the Physical Extentsize (PE) size, which by default is 4MB.
    # Create a encrypted volume
    echo -n "${passphrase}" | cryptsetup luksFormat --type luks2 --cipher aes-xts-plain64 /dev/mapper/system-rootfs -
    proc::hook_exit "[[ -e /dev/mapper/rootfs ]] && dmsetup remove rootfs"
    echo -n "${passphrase}" | cryptsetup open /dev/mapper/system-rootfs rootfs -
    # Copy rootfs content to the encrypted volume
    dd status=progress "if=${rootfs_file_path}" of=/dev/mapper/rootfs bs=4M
}

step::setup_rootfs_hash_lv() {
    local boot_part=$1
    local rootfs_hash_file_path="${workdir}/rootfs_hash.img"
    veritysetup format /dev/mapper/rootfs "${rootfs_hash_file_path}" --format=1 --hash=sha256 |
        tee "${workdir}/rootfs_hash.status" |
        gawk '(/^Root hash:/ && $NF ~ /^[0-9a-fA-F]+$/) { print $NF; }' \
            >"${workdir}/rootfs_hash.roothash"
    dmsetup remove rootfs
    cat "${workdir}/rootfs_hash.status"

    local rootfs_hash_size_in_byte
    rootfs_hash_size_in_byte=$(stat --printf="%s" "${rootfs_hash_file_path}")
    proc::hook_exit "[[ -e /dev/mapper/system-rootfs_hash ]] && disk::dm_remove_all ${device}"
    proc::exec_subshell_flose_fds lvcreate -n rootfs_hash --size ${rootfs_hash_size_in_byte}B system
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
    disk::umount_wait_busy ${boot_mount_point}
}

main() {
    if [ "$(id -u)" != "0" ]; then
        echo "This script must be run as root" 1>&2
        exit 1
    fi

    local device
    local input_file
    local output_file
    local config_dir
    local passphrase
    local packages=()

    while [[ "$#" -gt 0 ]]; do
        case $1 in
        -d | --device)
            operate_on_device=true
            device="$2"
            shift 2
            ;;
        --in)
            operate_on_device=false
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
        --passphrase)
            passphrase="$2"
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

    local operate_on_device
    if [ -n "${device:-}" ]; then
        if [ -n "${input_file:-}" ] || [ -n "${output_file:-}" ]; then
            proc::fatal "Cannot specify both --device and --in/--out file"
        fi
        operate_on_device=true
    elif [ -n "${input_file:-}" ] && [ -n "${output_file:-}" ]; then
        operate_on_device=false
    else
        proc::fatal "Must specify either --device or --in/--out file"
    fi

    if [ -z "${config_dir:-}" ]; then
        proc::fatal "Must specify --config-dir"
    elif [ ! -d "${config_dir}" ]; then
        proc::fatal "Cryptpilot config dir ${config_dir} does not exist"
    fi

    if [ -z "${passphrase:-}" ]; then
        proc::fatal "Must specify --passphrase"
    fi

    if [ "${operate_on_device}" = true ]; then
        if [ ! -b "${device}" ]; then
            proc::fatal "Input device $device does not exist"
        fi

        # In a better way to notice user that the data on the device may be lost if the operation is failed or canceled.
        echo "WARNING: This operation will overwrite data on the device ($device), and may cause data loss if the operation is failed or canceled. Make sure you have create a backup of the data !!!"
        while true; do
            read -p "Are you sure you want to continue? (y/n) " yn
            case $yn in
            [y]*)
                echo "Starting to convert the disk ..."
                break
                ;;
            [n]*)
                echo "Operation canceled."
                exit
                ;;
            *) echo "Please answer 'y' or 'n'." ;;
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

    echo "[ 0 ] Checking for required tools"
    yum install -y qemu-img cryptsetup veritysetup lvm2 parted grub2-tools e2fsprogs lsof

    #
    # 1. Prepare disk
    #
    echo "[ 1 ] Prepare disk"

    if [ "$operate_on_device" = true ]; then
        echo "Using device: $device"
    else
        echo "Using input file: $input_file"
        device="$(disk::get_available_nbd)" || proc::fatal "no free NBD device"

        local work_file="${input_file}.work"
        if [ -f "${work_file}" ]; then
            if flock --exclusive --nonblock "${work_file}"; then
                echo "File ${work_file} is locked by another process, maybe another cryptpilot instance is using it. Please stop it and try again."
                exit 1
            else
                echo "Temporary file ${work_file} already exists, delete it now"
                rm -f "${work_file}"
            fi
        fi

        echo "Copying ${input_file} to ${work_file}"
        proc::hook_exit "rm -f ${work_file}"
        cp "${input_file}" "${work_file}"
        proc::hook_exit "qemu-nbd --disconnect ${device}"
        qemu-nbd --connect="${device}" --discard=on --detect-zeroes=unmap "${work_file}"
        sleep 2
        echo "Mapped to NBD device ${device}"
    fi

    disk::assert_disk_not_busy "${device}"

    local efi_part_num
    efi_part_num=$(disk::find_efi_partition "${device}")
    local efi_part
    efi_part="${device}p${efi_part_num}"

    local rootfs_orig_part_num
    rootfs_orig_part_num=$(disk::find_rootfs_partition "${device}")
    local rootfs_orig_part
    rootfs_orig_part="${device}p${rootfs_orig_part_num}"
    rootfs_orig_start_sector=$(parted $device --script -- unit s print | grep "^ ${rootfs_orig_part_num}" | awk '{print $2}' | sed 's/s//')

    local sector_size
    sector_size=$(blockdev --getss "${device}")
    echo "Information about the disk:"
    echo "    Device: $device"
    echo "    Sector size: ${sector_size} bytes"
    echo "    EFI partition: $efi_part"
    echo "    Rootfs partition: $rootfs_orig_part"

    #
    # 2. Extracting /boot to boot partition
    #
    echo "[ 2 ] Extracting /boot to boot partition"
    local boot_file_path
    step::extract_boot_part_from_rootfs "$rootfs_orig_part"

    #
    # 3. Update rootfs and initrd
    #
    echo "[ 3 ] Update rootfs and initrd"
    step:update_rootfs_and_initrd "${efi_part}" "${boot_file_path}"

    #
    # 4. Shrinking rootfs and extract
    #
    echo "[ 4 ] Shrinking rootfs and extract"
    local boot_file_path
    step::shrink_and_extract_rootfs_part "${rootfs_orig_part}"

    #
    # 5. Create a boot partition
    #
    echo "[ 5 ] Creating boot partition"
    local boot_part_end_sector
    local boot_part
    step::create_boot_part "${boot_file_path}" "${rootfs_orig_start_sector}"

    #
    # 6. Creating lvm partition
    #
    echo "[ 6 ] Creating lvm partition"
    step::create_lvm_part "$((boot_part_end_sector + 1))"

    #
    # 7. Setting up rootfs logical volume
    #
    echo "[ 7 ] Setting up rootfs logical volume"
    step::setup_rootfs_lv "${passphrase}" "${rootfs_file_path}"

    #
    # 8. Setting up rootfs hash volume
    #
    echo "[ 8 ] Setting up rootfs hash volume"
    step::setup_rootfs_hash_lv "${boot_part}"

    #
    # 9. Cleaning up
    #
    echo "[ 9 ] Cleaning up"
    disk::dm_remove_all "${device}"
    blockdev --flushbufs "${device}"

    if [ "${operate_on_device}" == true ]; then
        echo "Everything done, the device is ready to use: ${device}"
    else
        #
        # 10. Generating new image file
        #
        echo "[ 10 ] Generating new image file"
        qemu-nbd --disconnect "${device}"
        # check suffix of the output file
        local output_file_suffix=${output_file##*.}
        if [[ "${output_file_suffix}" == "vhd" ]]; then
            qemu-img convert -p -O vpc "${work_file}" "${output_file}"
        elif [[ ${output_file_suffix} == "qcow2" ]]; then
            # It is not worth to enable the compression option "-c", since it does increase the compression time.
            qemu-img convert -p -O qcow2 "${work_file}" "${output_file}"
        else
            echo "Unknown output file suffix: ${output_file_suffix}"
            echo "Generating qcow2 file by default"
            qemu-img convert -p -O qcow2 "${work_file}" "${output_file}"
        fi

        echo "Everything done, the new disk image is ready to use: ${output_file}"
    fi

}

main "$@"
