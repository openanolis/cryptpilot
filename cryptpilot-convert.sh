#!/bin/bash

set -e # exit on error
set -u # exit when variable not set

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

# https://stackoverflow.com/a/7287873/15011229
#
# note: printf is used instead of echo to avoid backslash
# processing and to properly handle values that begin with a '-'.

log() { printf '%s\n' "$*"; }
error() { log "ERROR: $*" >&2; }
fatal() {
    error "$@"
    exit 1
}

trap_cmd_pre() {
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
trap_add() {
    trap_add_cmd=$1
    shift || fatal "${FUNCNAME} usage error"

    # get the num of args
    if [[ $# -eq 0 ]]; then
        fatal "trap name not specitied"
    fi

    for trap_add_name in "$@"; do
        trap -- "$(
            # print the new trap command
            printf 'trap_cmd_pre\n%s\n' "${trap_add_cmd}"
            # helper fn to get existing trap command from output
            # of trap -p
            extract_trap_cmd() { printf '%s\n' "${3:-:;}" | sed '/trap_cmd_pre/d'; }
            # print existing trap command with newline
            eval "extract_trap_cmd $(trap -p "${trap_add_name}") "
        )" "${trap_add_name}" ||
            fatal "unable to add to trap ${trap_add_name}"
    done
}
# set the trace attribute for the above function.  this is
# required to modify DEBUG or RETURN traps because functions don't
# inherit them unless the trace attribute is set
declare -f -t trap_add

hook_exit() {
    trap_add "$1" EXIT INT QUIT TERM
}

declare -f -t hook_exit

hook_exit 'trap "" EXIT' # some shells will call EXIT after the INT handler

assert_disk_not_busy() {
    # Check if lvm is using the disk
    if [[ $(lsblk --list -o TYPE $1 | awk 'NR>1 {print $1}' | grep -v -E '(part|disk)' | wc -l) -gt 0 ]]; then
        fatal "The disk is in use, please stop it first."
    fi

    if [[ $(lsblk -l -o MOUNTPOINT $1 | awk 'NR>1 {print $1}') != "" ]]; then
        fatal "The disk is some where mounted, please unmount it first."
    fi
}

dm_remove_all() {
    local device="$1"
    for dm_name in $(cat <(lsblk "$device" --list | awk 'NR>1 {print $1}') <(dmsetup ls | awk '{print $1}') | sort | uniq -d); do
        dmsetup remove "$dm_name"
    done
}

# To avoid locale issues.
export LC_ALL=C

if [ "$(id -u)" != "0" ]; then
    echo "This script must be run as root" 1>&2
    exit 1
fi

print_help_and_exit() {
    echo "Usage:"
    echo "    $0 <input_file> <output_file> <cryptpilot_config_dir> <rootfs_encrypt_passphrase>"
    echo "    $0 <disk> <cryptpilot_config_dir> <rootfs_encrypt_passphrase>"
    echo ""
    echo "Example:"
    echo "    $0 ./aliyun_3_x64_20G_nocloud_alibase_20240819.vhd ./aliyun_3_x64_20G_nocloud_alibase_20240819_cc.vhd ./cryptpilot_config AAAaaawewe222"
    echo "    $0 /dev/nvme1n1 ./cryptpilot_config AAAaaawewe222"
    exit 1
}

cryptpilot_rpm_path=/root/rpmbuild/RPMS/x86_64/cryptpilot-0.2.0-1.al8.x86_64.rpm
dcap_repo=https://enclave-cn-beijing.oss-cn-beijing.aliyuncs.com/repo/alinux/enclave-expr.repo

# the size is currently fixed with 512MB
boot_part_size="512M"
# alignment to 2048 sectors creating a new partition
sector_alignment=2048

align_start_sector() {
    local start_sector=$1
    if ((start_sector % sector_alignment != 0)); then
        start_sector=$((((start_sector - 1) / sector_alignment + 1) * sector_alignment))
    fi
    echo $start_sector
}

# https://unix.stackexchange.com/a/312273
nbd-available() {
    [[ $(blockdev --getsize64 $1) -eq 0 ]]
}

get-available-nbd() {
    modprobe nbd max_part=8
    local a
    for a in /dev/nbd[0-9] /dev/nbd[1-9][0-9]; do
        nbd-available "$a" || continue
        echo "$a"
        return 0
    done
    return 1
}

umount_wait_busy() {
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

operate_on_device=

if [ $# -eq 3 ]; then
    device=$1
    config_dir=$2
    passphrase=$3 # the passphrase used to encrypt the rootfs

    if [ ! -b "$device" ]; then
        fatal "Input device $device does not exist"
    fi

    if [ ! -d "${config_dir}" ]; then
        fatal "Cryptpilot config dir ${config_dir} does not exist"
    fi

    operate_on_device=true
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
elif [ $# -eq 4 ]; then
    input_file=$1
    output_file=$2
    config_dir=$3
    passphrase=$4 # the passphrase used to encrypt the rootfs

    if [ ! -f "$input_file" ]; then
        fatal "Input file $input_file does not exist"
    fi

    if [ ! -d "${config_dir}" ]; then
        fatal "Cryptpilot config dir ${config_dir} does not exist"
    fi
else
    print_help_and_exit
fi

# Install trap to collect error info on exit with error
hook_exit ":;"

#
# 1. Prepare disk
#
echo "[ 1 ] Prepare disk"

if [ "$operate_on_device" = true ]; then
    echo "Using device: $device"
else
    echo "Using input file: $input_file"
    device="$(get-available-nbd)" || fatal "no free NBD device"

    work_file=${input_file}.work
    if [ -f "$work_file" ]; then
        if flock --exclusive --nonblock $work_file; then
            echo "File $work_file is locked by another process, maybe another cryptpilot instance is using it. Please stop it and try again."
            exit 1
        else
            echo "Temporary file $work_file already exists, delete it now"
            rm -f $work_file
        fi
    fi

    echo "Copying $input_file to $work_file"
    hook_exit "rm -f ${work_file}"
    cp $input_file $work_file
    hook_exit "qemu-nbd --disconnect $device"
    qemu-nbd --connect=$device --discard=on --detect-zeroes=unmap $work_file
    sleep 2
    echo "Mapped to NBD device $device"
fi

assert_disk_not_busy $device

# Determine the right partition number by checking the partition table with partition label
find_rootfs_partition() {
    local part_num=1
    while true; do
        local part_path=${device}p${part_num}
        if [ ! -b "$part_path" ]; then
            echo "Cannot find rootfs partition" >&2
            return 1
        fi
        local label=$(blkid -o value -s LABEL $part_path)
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

find_efi_partition() {
    # find efi partition by PARTLABEL
    local part_num=1
    while true; do
        local part_path=${device}p${part_num}
        if [ ! -b "$part_path" ]; then
            break # try again with another method
        fi
        local part_label=$(blkid -o value -s PARTLABEL $part_path)
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
        local sec_type=$(blkid -o value -s SEC_TYPE $part_path)
        local part_type=$(blkid -o value -s TYPE $part_path)
        if [[ "$sec_type" == "msdos" ]] && [[ "$part_type" == "vfat" ]]; then
            echo "$part_num"
            return 0
        fi
        part_num=$((part_num + 1))
    done
}

efi_part_num=$(find_efi_partition)
efi_part=${device}p${efi_part_num}

rootfs_orig_part_num=$(find_rootfs_partition)
rootfs_orig_part=${device}p${rootfs_orig_part_num}

sector_size=$(blockdev --getss $device)
echo "Information about the disk:"
echo "    Device: $device"
echo "    Sector size: ${sector_size} bytes"
echo "    EFI partition: $efi_part"
echo "    Rootfs partition: $rootfs_orig_part"

# init a tmp workdir with mktemp
workdir=$(mktemp -d "/tmp/.cryptpilot-convert-XXXXXXXX")
hook_exit "rm -rf ${workdir}"

#
# 2. Extracting /boot to boot partition
#
echo "[ 2 ] Extracting /boot to boot partition"
boot_file_path=${workdir}/boot.img
fallocate -l $boot_part_size $boot_file_path
yes | mkfs.ext4 $boot_file_path
# get uuid of the boot image
boot_uuid=$(blkid -o value -s UUID $boot_file_path)
boot_mount_point=${workdir}/boot
mkdir -p $boot_mount_point
hook_exit "mountpoint -q ${boot_mount_point} && umount_wait_busy ${boot_mount_point}"
mount $boot_file_path $boot_mount_point

# mount the rootfs
rootfs_mount_point=${workdir}/rootfs
mkdir -p $rootfs_mount_point
hook_exit "mountpoint -q ${rootfs_mount_point} && umount_wait_busy ${rootfs_mount_point}"
mount $rootfs_orig_part $rootfs_mount_point

# extract the /boot content to a boot.img
cp -a ${rootfs_mount_point}/boot/. ${boot_mount_point}
find ${rootfs_mount_point}/boot/ -mindepth 1 -delete

# When booting alinux3 image with legecy BIOS support in UEFI ECS instance, the real grub.cfg is located at /boot/grub2/grub.cfg, and will be searched by matching path.
# i.e.:
# search --no-floppy --set prefix --file /boot/grub2/grub.cfg
#
# Here we create a symlink to the boot directory so that grub can find it's grub.cfg.
ln -s -f . ${boot_mount_point}/boot

#
# 3. Installing cryptpilot onto rootfs and initrd
#
echo "[ 3 ] Installing cryptpilot onto rootfs and initrd"
hook_exit "mountpoint -q ${rootfs_mount_point}/dev && umount ${rootfs_mount_point}/dev"
mount -t devtmpfs devtmpfs ${rootfs_mount_point}/dev
hook_exit "mountpoint -q ${rootfs_mount_point}/dev/pts && umount ${rootfs_mount_point}/dev/pts"
mount -t devpts devpts ${rootfs_mount_point}/dev/pts
hook_exit "mountpoint -q ${rootfs_mount_point}/proc && umount ${rootfs_mount_point}/proc"
mount -t proc proc ${rootfs_mount_point}/proc
hook_exit "mountpoint -q ${rootfs_mount_point}/run && umount ${rootfs_mount_point}/run"
mount -t tmpfs tmpfs ${rootfs_mount_point}/run
hook_exit "mountpoint -q ${rootfs_mount_point}/sys && umount ${rootfs_mount_point}/sys"
mount -t sysfs sysfs ${rootfs_mount_point}/sys
# mount bind boot
hook_exit "mountpoint -q ${rootfs_mount_point}/boot && umount ${rootfs_mount_point}/boot"
mount --bind ${boot_mount_point} ${rootfs_mount_point}/boot
# also mount the EFI part
hook_exit "mountpoint -q ${rootfs_mount_point}/boot/efi && umount ${rootfs_mount_point}/boot/efi"
mount $efi_part ${rootfs_mount_point}/boot/efi

# install cryptpilot.rpm
echo "Installing cryptpilot"
yum-config-manager --installroot=${rootfs_mount_point} --add-repo ${dcap_repo}
yum --installroot=${rootfs_mount_point} install -y ${cryptpilot_rpm_path}
yum --installroot=${rootfs_mount_point} clean all

# copy cryptpilot config
echo "Copying cryptpilot config from /etc/cryptpilot to target rootfs"
mkdir -p ${rootfs_mount_point}/etc/cryptpilot/
cp -a ${config_dir}. ${rootfs_mount_point}/etc/cryptpilot/

# update /etc/fstab
echo "Updating /etc/fstab"
boot_mount_line="UUID=${boot_uuid} /boot ext4 defaults 0 2"
root_mount_line_number=$(grep -n -E '^[[:space:]]*[^#][^[:space:]]+[[:space:]]+/[[:space:]]+.*$' "${rootfs_mount_point}/etc/fstab" | head -n 1 | cut -d: -f1)
if [ ! -n "$root_mount_line_number" ]; then
    fatal "Cannot find mount for / in /etc/fstab"
fi
boot_mount_insert_line_number=$((root_mount_line_number + 1))
sed -i "${boot_mount_insert_line_number}i${boot_mount_line}" "${rootfs_mount_point}/etc/fstab"

# update initrd
echo "Updating initrd and grub2"
chroot ${rootfs_mount_point} bash -c "$(
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
umount ${rootfs_mount_point}/boot/efi
umount ${rootfs_mount_point}/boot
umount ${rootfs_mount_point}/sys
umount ${rootfs_mount_point}/run
umount ${rootfs_mount_point}/proc
umount ${rootfs_mount_point}/dev/pts
umount ${rootfs_mount_point}/dev

umount_wait_busy ${boot_mount_point}
umount_wait_busy ${rootfs_mount_point}

#
# 4. Shrinking rootfs to smallest possible size
#
echo "[ 4 ] Shrinking rootfs to smallest possible size"
e2fsck -y -f $rootfs_orig_part
resize2fs -M $rootfs_orig_part # adjust file system content, all move to front
# TODO: support filesystem other than ext4
rootfs_orig_block_size=$(dumpe2fs $rootfs_orig_part 2>/dev/null | grep 'Block size' | awk '{print $3}')
rootfs_orig_block_count=$(dumpe2fs $rootfs_orig_part 2>/dev/null | grep 'Block count' | awk '{print $3}')
rootfs_orig_size_in_bytes=$((rootfs_orig_block_size * rootfs_orig_block_count))
rootfs_orig_size_in_sector=$((rootfs_orig_block_size * rootfs_orig_block_count / sector_size))
echo "Information about the shrinked rootfs:"
echo "    Block size: $rootfs_orig_block_size"
echo "    Block count: $rootfs_orig_block_count"
echo "    Size in Bytes: $rootfs_orig_size_in_bytes"
echo "    Size in Sector: $rootfs_orig_size_in_sector"

rootfs_orig_start_sector=$(parted $device --script -- unit s print | grep "^ ${rootfs_orig_part_num}" | awk '{print $2}' | sed 's/s//')
playground_start_sector=$rootfs_orig_start_sector
rootfs_orig_new_end_sector=$((playground_start_sector + rootfs_orig_size_in_sector - 1))
dd status=progress if=/dev/zero of=$rootfs_orig_part count=$(($(blockdev --getsize64 ${rootfs_orig_part}) - ${rootfs_orig_size_in_bytes})) iflag=count_bytes seek=$rootfs_orig_size_in_bytes oflag=seek_bytes bs=256M # Clean the freed space with zero, so that the qemu-img convert would generate smaller image
echo Yes | parted $device ---pretend-input-tty resizepart ${rootfs_orig_part_num} ${rootfs_orig_new_end_sector}s                                                                                                      # Resize the third partition to the calculated size
partprobe $device                                                                                                                                                                                                     # Inform the OS of partition table changes
[[ $rootfs_orig_size_in_bytes == $(blockdev --getsize64 $rootfs_orig_part) ]] || echo "Wrong size, something wrong in the script"

end_sector_on_device=$(parted $device unit s p free | grep 'Free Space' | tail -n 1 | awk '{print $2}' | sed 's/s//')
rootfs_bak_end_sector=${end_sector_on_device}
rootfs_bak_start_sector=$((end_sector_on_device - rootfs_orig_size_in_sector + 1))
echo "Moving rootfs to the end of the disk ($rootfs_bak_start_sector ... $rootfs_bak_end_sector sectors)"
parted $device --script -- mkpart rootfs-bak ext4 ${rootfs_bak_start_sector}s ${rootfs_bak_end_sector}s
partprobe $device
rootfs_bak_part_num=$((rootfs_orig_part_num + 1))
rootfs_bak_part=${device}p${rootfs_bak_part_num}
[[ $(blockdev --getsize64 $rootfs_bak_part) == $(blockdev --getsize64 $rootfs_orig_part) ]] || echo "Wrong size, something wrong in the script"
dd status=progress if=$rootfs_orig_part of=$rootfs_bak_part bs=64M
dd status=progress if=/dev/zero of=$rootfs_orig_part count=$(blockdev --getsize64 $rootfs_orig_part) iflag=count_bytes bs=64M
# Delete the original rootfs partition
parted $device --script -- rm ${rootfs_orig_part_num}
partprobe $device

#
# 5. Create a boot partition
#
echo "[ 5 ] Creating boot partition"
boot_part_num=${rootfs_orig_part_num}
boot_part="${device}p${boot_part_num}"
boot_size_in_bytes=$(stat --printf="%s" $boot_file_path)
boot_size_in_sector=$((boot_size_in_bytes / sector_size))
boot_start_sector=$(align_start_sector ${playground_start_sector})
boot_end_sector=$((boot_start_sector + boot_size_in_sector - 1))
echo "Creating boot partition ($boot_start_sector ... $boot_end_sector sectors)"
parted $device --script -- mkpart boot ext4 ${boot_start_sector}s ${boot_end_sector}s
[[ $boot_size_in_bytes == $(blockdev --getsize64 $boot_part) ]] || echo "Wrong size, something wrong in the script"
dd status=progress if=$boot_file_path of=$boot_part bs=4M
rm -f $boot_file_path

#
# 6. Creating lvm partition
#
echo "[ 6 ] Creating lvm partition"
lvm_part_num=$((rootfs_orig_part_num + 2))
lvm_part="${device}p${lvm_part_num}"
lvm_start_sector=$((boot_end_sector + 1))
lvm_start_sector=$(align_start_sector ${lvm_start_sector})
lvm_end_sector=$((rootfs_bak_start_sector - 1))
echo "Creating lvm partition as LVM PV ($lvm_start_sector ... $lvm_end_sector sectors)"
parted $device --script -- mkpart system ${lvm_start_sector}s ${lvm_end_sector}s
parted $device --script -- set ${lvm_part_num} lvm on
partprobe $device
pvcreate $lvm_part
vgcreate system $lvm_part

#
# 7. Setting up rootfs logical volume
#
echo "[ 7 ] Setting up rootfs logical volume"
rootfs_lv_size_in_bytes=$((rootfs_orig_size_in_bytes + 16 * 1024 * 1024)) # original rootfs partition size plus LUKS2 header size
echo "Creating rootfs logical volume"
hook_exit "[[ -e /dev/mapper/system-rootfs ]] && dm_remove_all ${device}"
lvcreate -n rootfs --size ${rootfs_lv_size_in_bytes}B system # Note that the real size will be a little bit larger than the specified size, since they will be aligned to the Physical Extentsize (PE) size, which by default is 4MB.
# Create a encrypted volume
echo -n "$passphrase" | cryptsetup luksFormat --type luks2 --cipher aes-xts-plain64 /dev/mapper/system-rootfs -
hook_exit "[[ -e /dev/mapper/rootfs ]] && dmsetup remove rootfs"
echo -n "$passphrase" | cryptsetup open /dev/mapper/system-rootfs rootfs -
# Copy rootfs content to the encrypted volume
dd status=progress if=$rootfs_bak_part of=/dev/mapper/rootfs bs=4M

# Remove the rootfs-bak partition
dd status=progress if=/dev/zero of=$rootfs_bak_part count=$(blockdev --getsize64 $rootfs_bak_part) iflag=count_bytes bs=256M
parted $device --script -- rm ${rootfs_bak_part_num}
partprobe $device
unset rootfs_bak_part
unset rootfs_bak_part_num

#
# 8. Setting up rootfs hash volume
#
echo "[ 8 ] Setting up rootfs hash volume"
rootfs_hash_file_path=${workdir}/rootfs_hash.img
veritysetup format /dev/mapper/rootfs $rootfs_hash_file_path --format=1 --hash=sha256 |
    tee "${workdir}/rootfs_hash.status" |
    gawk '(/^Root hash:/ && $NF ~ /^[0-9a-fA-F]+$/) { print $NF; }' \
        >"${workdir}/rootfs_hash.roothash"
dmsetup remove rootfs
cat ${workdir}/rootfs_hash.status # show status
rootfs_hash_size_in_byte=$(stat --printf="%s" $rootfs_hash_file_path)
hook_exit "[[ -e /dev/mapper/system-rootfs_hash ]] && dm_remove_all ${device}"
lvcreate -n rootfs_hash --size ${rootfs_hash_size_in_byte}B system
dd status=progress if=$rootfs_hash_file_path of=/dev/mapper/system-rootfs_hash bs=4M
rm -f ${rootfs_hash_file_path}
unset rootfs_hash_file_path
dm_remove_all ${device}
# Recording rootfs hash in boot partition
hook_exit "mountpoint -q ${boot_mount_point} && umount_wait_busy ${boot_mount_point}"
mount $boot_part $boot_mount_point
mkdir -p $boot_mount_point/cryptpilot/
cat <<EOF >$boot_mount_point/cryptpilot/metadata.toml
type = 1
root_hash = "$(cat ${workdir}/rootfs_hash.roothash)"
EOF

#
# 9. Fix partation number
#
echo "[ 9 ] Fix partation number"
parted $device --script -- rm ${lvm_part_num}
parted $device --script -- mkpart system ${lvm_start_sector}s ${lvm_end_sector}s
lvm_part_num=$((rootfs_orig_part_num + 1))
parted $device --script -- set ${lvm_part_num} lvm on
partprobe $device
dm_remove_all $device

if [ "$operate_on_device" == true ]; then
    echo "Everything done, the device is ready to use: $device"
else
    #
    # 10. Generating new image file
    #
    echo "[ 10 ] Generating new image file"
    qemu-nbd --disconnect $device
    # check suffix of the output file
    output_file_suffix=${output_file##*.}
    if [[ ${output_file_suffix} == "vhd" ]]; then
        qemu-img convert -p -O vpc ${work_file} ${output_file}
    elif [[ ${output_file_suffix} == "qcow2" ]]; then
        # It is not worth to enable the compression option "-c", since it does increase the compression time.
        qemu-img convert -p -O qcow2 ${work_file} ${output_file}
    else
        echo "Unknown output file suffix: ${output_file_suffix}"
        echo "Generating qcow2 file by default"
        qemu-img convert -p -O qcow2 ${work_file} ${output_file}
    fi

    echo "Everything done, the new disk image is ready to use: ${output_file}"
fi
