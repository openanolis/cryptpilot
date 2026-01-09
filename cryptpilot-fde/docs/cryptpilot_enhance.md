# cryptpilot-enhance

## NAME

cryptpilot-enhance — Harden virtual machine disk images before encryption

## SYNOPSIS

```bash
cryptpilot-enhance --mode MODE --image IMAGE_PATH [--ssh-key PUBKEY_FILE]
```

## DESCRIPTION

`cryptpilot-enhance` performs security hardening on offline VM disk images (e.g., QCOW2) using `virt-customize`. All operations are executed in a single guest session to minimize startup overhead, making it suitable for use in secure build pipelines and pre-encryption workflows.

The script applies system-level configurations including removal of cloud agents, service deactivation, user account cleanup, SSH hardening, and sensitive data erasure—without booting the target operating system.

## OPTIONS

`--mode MODE`  
    Set hardening level. Supported values:  
    - `full`: Maximum security. Removes SSH server and enforces strict access controls.  
    - `partial`: Retains SSH with public key authentication only; allows remote administration under hardened conditions.

`--image IMAGE_PATH`  
    Path to the disk image file (QCOW2 or RAW format). The file must exist and be readable.

`--ssh-key PUBKEY_FILE`  
    (Optional) Path to an OpenSSH public key file. Used in `partial` mode to inject the key into `root`'s `~/.ssh/authorized_keys`.

`--help`  
    Display usage information and exit.

## HARDENING ACTIONS

### Common Actions (applied in both modes)

- Uninstall Alibaba Cloud Assistant:
  - Stop and remove `aliyun.service` and `assist_daemon`
  - Remove associated binaries and configuration files
- Uninstall Aegis (Cloud Security Center):
  - Download and execute official uninstall script
- Disable `rpcbind`:
  - Stop, disable, and mask `rpcbind.service` and `rpcbind.socket`
- Remove `cloud-init`:
  - Execute `yum remove -y cloud-init`
- User account cleanup:
  - Lock passwords for `root` and `admin` by setting `!!` in `/etc/shadow`
  - Delete all non-exempt user accounts with interactive shells and active passwords
  - Clean up home directories ending in `.DEL`
- Clear shell history:
  - Execute `history -c && history -w` to erase command history

### Mode-Specific Actions

**Mode: full**
- Remove SSH server: `yum remove -y openssh-server`

**Mode: partial**
- Secure SSH configuration:
  - `PasswordAuthentication no`
  - `PubkeyAuthentication yes`
  - `PermitRootLogin prohibit-password`
  - `X11Forwarding no`
  - `AllowTcpForwarding no`
- Inject public key into `root`'s `authorized_keys` if provided

## EXAMPLES

Apply full hardening to an image:

```bash
./cryptpilot-enhance \
  --mode full \
  --image ./server-disk.qcow2
```

Apply partial hardening with SSH key injection:

```bash
./cryptpilot-enhance \
  --mode partial \
  --image ./server-disk.qcow2 \
  --ssh-key ~/.ssh/id_rsa.pub
```

## REQUIREMENTS

- `libguestfs-tools` package installed
- `virt-customize` available in `$PATH`
- Sufficient privileges to access and modify disk image files

Tested on CentOS/RHEL 7/8/9 systems. May require adaptation for other distributions.

By default, `virt-customize` uses the libvirt backend which requires a running libvirtd daemon. If you encounter an error like:

```
libvirt: XML-RPC error : Failed to connect socket to '/var/run/libvirt/libvirt-sock': No such file or directory
virt-customize: error: libguestfs error: could not connect to libvirt (URI = qemu:///system): Failed to connect socket to '/var/run/libvirt/libvirt-sock': No such file or directory
```

You can work around this by setting the environment variable `LIBGUESTFS_BACKEND=direct`:

```bash
LIBGUESTFS_BACKEND=direct ./cryptpilot-enhance --mode partial --image ./server-disk.qcow2
```

## SECURITY NOTES

- This script modifies the disk image permanently.
- Always test on a copy of the original image.
- After hardening, recovery options may be limited; ensure alternative access methods are in place when needed.

## SEE ALSO

- `virt-customize(1)`
- `libguestfs-tools(1)`

## LICENSE

Apache License. See LICENSE file for details.