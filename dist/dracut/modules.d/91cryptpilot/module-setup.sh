#!/bin/bash

check() {
        # By default we don't install
        # https://man7.org/linux/man-pages/man7/dracut.modules.7.html
        return 255
}

install() {
        set -e
        set -u
        # TODO: simplify this
        inst_multiple cryptsetup veritysetup mkfs.ext4 mkfs.vfat mkfs.xfs mkswap base64
        inst_multiple vgchange lvcreate
        inst_multiple blkid lsblk findmnt
        inst_multiple dd tail grep sort
        inst_multiple awk sed pvs growpart sfdisk lvm lvextend mountpoint
        # required by 'file' command
        inst_multiple file
        inst_multiple modprobe
        inst_simple /usr/share/misc/magic
        inst_simple /usr/share/misc/magic.mgc
        # For debug only
        # inst_multiple curl nc ip find systemctl journalctl ifconfig lsblk df
        inst_multiple cryptpilot

        # TODO: It would be better compatible to use the same network service in initrd as in system. So here we enable NetworkManager in initrd since the Alinux3 OS is using NetworkManager in system. But it would be better to have a more general way to select network service to be enabled.
        # Enable NetworkManager
        # shellcheck disable=SC2154
        echo rd.neednet=1 ip=dhcp >>"$initdir"/etc/cmdline.d/35-cryptpilot.conf

        # The dracut version in Alinux3 yum repo is 049 where NetworkManager is not a systemd service and there is no nm-wait-online-initrd.service to provide "network-online.target". (Should be ready after version 053: https://github.com/dracutdevs/dracut/pull/1052). It is hard to back port a nm-wait-online-initrd.service since the lack of D-BUS required by nm-online in initrd. So we use a fake "wait-network-online.service" service to replace it.
        # shellcheck disable=SC2154
        if [ ! -e "$moddir"/../35network-manager/nm-wait-online-initrd.service ]; then
                inst_hook initqueue/online 95 "$moddir"/initrd-trigger-network-online.sh
                inst_simple "$moddir"/initrd-wait-network-online.sh /usr/lib/cryptpilot/bin/initrd-wait-network-online.sh
                inst_simple "$moddir"/initrd-wait-network-online.service /usr/lib/systemd/system/initrd-wait-network-online.service
                systemctl --root "$initdir" enable initrd-wait-network-online.service
        fi
        inst_simple "$moddir"/cryptpilot-fde-before-sysroot.service /usr/lib/systemd/system/cryptpilot-fde-before-sysroot.service
        inst_simple "$moddir"/cryptpilot-fde-after-sysroot.service /usr/lib/systemd/system/cryptpilot-fde-after-sysroot.service
        systemctl --root "$initdir" enable cryptpilot-fde-after-sysroot.service
        systemctl --root "$initdir" enable cryptpilot-fde-before-sysroot.service

        # Install essential udev rules
        inst_simple "${dracutsysrootdir:-}"/usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules /usr/lib/udev/rules.d/12-cryptpilot-hide-intermediate-devices.rules

        # Install lvm config required by cryptpilot
        inst_simple "$moddir"/lvm.conf /usr/lib/cryptpilot/lvm/lvm.conf

        set +u
        set +e
}

installkernel() {
        # Install kernel modules regardless of the hostonly mode
        hostonly='' instmods dm-mod dm-crypt dm-integrity dm-verity authenc overlay
        hostonly='' instmods virtio-pci virtio-net net-failover
        hostonly='' instmods loop
}

depends() {
        echo crypt
        echo network
        # We need to install ssl ca certs for HTTPS support
        echo url-lib
        if require_binaries attestation-agent; then
                echo attestation-agent
        fi
        if require_binaries confidential-data-hub; then
                echo confidential-data-hub
        fi
}
