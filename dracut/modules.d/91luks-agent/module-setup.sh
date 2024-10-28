#!/bin/bash

check() {
        return 0
}

install() {
        inst_multiple curl nc ip find systemctl journalctl ifconfig lsblk df
        inst_simple "$moddir/luks-agent" "/usr/bin/luks-agent"
        inst_simple "$moddir/luks-agent.service" "/usr/lib/systemd/system/luks-agent.service"
        inst_simple "$moddir/kbs-root.crt" "/etc/kbs-root.crt"
        inst_simple "$moddir/attest-params.json" "/etc/attest-params.json"
        inst_simple "$moddir/kbs-client" "/usr/bin/kbs-client"
        # inst_simple "$moddir/systemd-cryptsetup" "/usr/bin/systemd-cryptsetup"
	systemctl --root "$initdir" add-wants sockets.target luks-agent.service # 2>/dev/null
        echo "vdb1 /dev/vdb1 /tmp/luks.sock" >> $initdir/etc/crypttab
}

installkernel() {
        instmods dm_mod dm_crypt virtio_net net_failover
}

depends() {
        echo crypt systemd-networkd network network-manager kernel-network-modules
}
