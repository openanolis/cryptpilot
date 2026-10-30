#!/bin/bash

set -euo pipefail

# Ensure consistent locale for parsing.
export LC_ALL=C

# ANSI color codes
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly PURPLE='\033[0;35m'
readonly CYAN='\033[0;36m'
readonly NC='\033[0m' # No Color

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

log::highlight() {
    printf "${PURPLE}📌 %s${NC}\n" "$*" >&2
}

proc::fatal() {
    log::error "$@"
    exit 1
}

usage() {
    cat <<'EOF'
Usage: cryptpilot-enhance [OPTIONS]

Securely harden a VM disk image before encryption using virt-customize.
All operations are performed in a single guest launch for optimal performance.

OPTIONS:
    --mode MODE           Hardening level: 'full' or 'partial'
                          - full:     Maximum security (removes SSH, locks all passwords)
                          - partial:  Retains SSH with key-only access, milder user restrictions
    --image IMAGE_PATH    Path to the disk image (qcow2 or raw format)
    --ssh-key KEY_FILE    (Optional) Public SSH key file to inject for root login (partial mode only)
    --help                Show this help message and exit

NOTES:
- Requires libguestfs-tools (virt-customize). Install via:
    CentOS/RHEL: yum install -y libguestfs-tools
    Ubuntu:      apt-get install -y libguestfs-tools
- By default, virt-customize uses the 'libvirt' backend (via libvirtd). For better performance
  or to avoid daemon dependencies, set the backend explicitly using LIBGUESTFS_BACKEND:
    - To use direct QEMU without libvirt (recommended for CI/containers):
        export LIBGUESTFS_BACKEND=direct
    - Example:
        LIBGUESTFS_BACKEND=direct cryptpilot-enhance --mode partial --image ./disk.qcow2
- The 'direct' backend avoids libvirtd overhead and works in containerized or minimal environments.
- Always test on a copy of the image before production use.
EOF
}

# Parse arguments
MODE="" IMAGE="" SSH_KEY=""

while [[ "$#" -gt 0 ]]; do
    case $1 in
    --mode)
        MODE="$2"
        shift 2
        ;;
    --image)
        IMAGE="$2"
        shift 2
        ;;
    --ssh-key)
        SSH_KEY="$2"
        shift 2
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        log::error "unknown argument $1"
        usage
        exit 1
        ;;
    esac
done

# Validate inputs
[[ -z "$MODE" ]] && {
    log::error "--mode is required."
    usage
    exit 1
}
[[ ! "$MODE" =~ ^(full|partial)$ ]] && {
    log::error "--mode must be 'full' or 'partial'."
    exit 1
}
[[ -z "$IMAGE" ]] && {
    log::error "--image is required."
    exit 1
}
[[ ! -f "$IMAGE" ]] && {
    log::error "image file not found: $IMAGE"
    exit 1
}
[[ "$MODE" == "partial" && -n "$SSH_KEY" && ! -f "$SSH_KEY" ]] && {
    log::error "SSH key file not found: $SSH_KEY"
    exit 1
}

# virt-customize is the tool this script drives. On Alinux 3 / RHEL 8 it ships in
# the "libguestfs-tools-c" package; on Alinux 4 it ships in "guestfs-tools",
# which provides "libguestfs-tools-c" and lands via the RPM's weak dependency.
# Image hardening is not essential to FDE convert/boot verification, so when
# virt-customize is absent OR unusable we skip hardening with a warning rather
# than fail the whole pipeline.
# Unusable means: supermin, which builds the appliance libguestfs boots, cannot
# detect the distro's package manager. Alinux 4 sets ID_LIKE=anolis and the
# supermin build has no anolis driver, so every virt-customize invocation fails
# instantly with "supermin: could not detect package manager" there.
if ! command -v virt-customize >/dev/null 2>&1; then
    log::warn "virt-customize is not installed; skipping image hardening for $IMAGE"
    log::warn "Install libguestfs-tools-c (Alinux 3/4) or libguestfs-tools (Ubuntu) to enable hardening."
    log::success "Hardening skipped (virt-customize unavailable)."
    exit 0
fi
if command -v supermin >/dev/null 2>&1 && ! supermin --list-drivers 2>/dev/null | grep -v 'not-detected' | grep -q 'detected'; then
    log::warn "virt-customize is installed but supermin cannot detect this distro's package manager"
    log::warn "(Alinux 4's ID_LIKE=anolis has no supermin driver); skipping image hardening for $IMAGE"
    log::success "Hardening skipped (virt-customize unusable in this environment)."
    exit 0
fi

log::info "Starting image hardening..."
log::highlight "Image: $IMAGE"
log::highlight "Mode: $MODE"

# Build virt-customize command (single execution)
VIRT_CMD=(
    virt-customize
    --format=qcow2
    -a "$IMAGE"
)

# Allow extra options to be passed via environment variable CRYPTPILOT_ENHANCE_VIRT_CUSTOMIZE_OPTS
# Example: CRYPTPILOT_ENHANCE_VIRT_CUSTOMIZE_OPTS="--memsize 4096 --smp 4" cryptpilot-enhance ...
if [[ -n "${CRYPTPILOT_ENHANCE_VIRT_CUSTOMIZE_OPTS:-}" ]]; then
    log::info "Appending extra virt-customize options: $CRYPTPILOT_ENHANCE_VIRT_CUSTOMIZE_OPTS"
    # Use read to safely split the variable into an array
    read -r -a _extra_opts <<< "$CRYPTPILOT_ENHANCE_VIRT_CUSTOMIZE_OPTS"
    VIRT_CMD+=("${_extra_opts[@]}")
fi

# Helper function to append --run-command
add_run_cmd() {
    VIRT_CMD+=(--run-command "$1")
}

# All commands below run inside the guest with the guest's /bin/sh, which is
# dash on Ubuntu, so they must stay POSIX sh (no arrays, no [[ ]]). Package
# operations dispatch on the guest's package manager so DEB guests go through
# apt. The Alibaba-agent steps are guarded no-ops on images without them.

# =============================
# 1. Uninstall Cloud Assistant Agent (Aliyun Assist)
# =============================
add_run_cmd '
if [ -d /usr/local/share/assist-daemon ] || [ -e /etc/systemd/system/aliyun.service ]; then
    /usr/local/share/assist-daemon/assist_daemon --stop 2>/dev/null || true
    systemctl stop aliyun.service 2>/dev/null || true
    /usr/local/share/assist-daemon/assist_daemon --delete 2>/dev/null || true
    rpm -qa 2>/dev/null | grep aliyun_assist | xargs -r rpm -e 2>/dev/null || true
    rm -rf /usr/local/share/aliyun-assist
    rm -rf /usr/local/share/assist-daemon
    rm -f /etc/systemd/system/aliyun.service
    rm -f /etc/init.d/aliyun-service
fi
'

# =============================
# 2. Uninstall Cloud Security Center (Aegis/AntiKnight)
# =============================
add_run_cmd '
# The uninstall script only makes sense where the agent is installed; plain
# Ubuntu images have none, and fetching it there would fail the run.
if [ -d /usr/local/aegis ]; then
    wget "http://update.aegis.aliyun.com/download/uninstall.sh" && chmod +x uninstall.sh && ./uninstall.sh
    rm -f /uninstall.sh
fi
'

# =============================
# 3. Disable and Mask rpcbind Service
# =============================
add_run_cmd '
# rpcbind is not installed on every distro (e.g. Ubuntu cloud images), so each
# step tolerates a missing unit instead of failing the whole hardening run.
systemctl stop rpcbind.service 2>/dev/null || true
systemctl disable rpcbind.service 2>/dev/null || true
systemctl mask rpcbind.service 2>/dev/null || true

systemctl stop rpcbind.socket 2>/dev/null || true
systemctl disable rpcbind.socket 2>/dev/null || true
systemctl mask rpcbind.socket 2>/dev/null || true
'

# =============================
# 4. Remove cloud-init
# =============================
add_run_cmd '
if command -v apt-get >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get purge -y cloud-init
    rm -rf /etc/cloud /var/lib/cloud
else
    yum remove -y cloud-init
fi
'

# =============================
# 5. Optional: Restrict or Remove SSH Service
# =============================
if [[ "$MODE" == "full" ]]; then
    add_run_cmd '
    if command -v apt-get >/dev/null 2>&1; then
        DEBIAN_FRONTEND=noninteractive apt-get purge -y openssh-server
    else
        yum remove -y openssh-server
    fi
    '
elif [[ "$MODE" == "partial" ]]; then
    add_run_cmd '
    # Disable password login, allow public key only
    sed -i "s/^#*PasswordAuthentication.*/PasswordAuthentication no/" /etc/ssh/sshd_config
    sed -i "s/^#*PubkeyAuthentication.*/PubkeyAuthentication yes/" /etc/ssh/sshd_config

    # Prevent root password login, but allow key-based login
    sed -i "s/^#*PermitRootLogin.*/PermitRootLogin prohibit-password/" /etc/ssh/sshd_config

    # Disable high-risk features
    sed -i "s/^#*X11Forwarding.*/X11Forwarding no/" /etc/ssh/sshd_config
    sed -i "s/^#*AllowTcpForwarding.*/AllowTcpForwarding no/" /etc/ssh/sshd_config
    '
fi

# =============================
# 6. Disable Linux User Password Login (Prevent Console Access)
# =============================
# shellcheck disable=SC2016
add_run_cmd '
# 1. Lock root and admin account passwords
echo "1. Locking root and admin account passwords..."
sed -i "s/\(^root:\)[^:]*/\1!!/" /etc/shadow
sed -i "s/\(^admin:\)[^:]*/\1!!/" /etc/shadow
echo "1. Root and admin account passwords locked!"

echo "2. Processing other user accounts (excluding root and admin)..."
# Delete interactive-shell accounts that still have a usable password; keep
# root/admin and anything already locked.
while IFS=: read -r username _ _ _ _ homedir user_shell; do
    case "$user_shell" in
    */bash|*/sh|*/zsh) ;;
    *) continue ;;
    esac

    case "$username" in
    root|admin) continue ;;
    esac

    # Read password field from shadow
    pass=$(grep "^$username:" /etc/shadow | cut -d: -f2)

    # If already locked (!, !!, *, etc.), skip deletion
    case "$pass" in
    "!"|"!!"|"*") continue ;;
    esac
    case "$pass" in
    *"!"*) continue ;;
    esac

    # Delete eligible user account and home directory
    echo "Removing account: $username"
    userdel -r "$username" 2>/dev/null || true
    if [ -d "$homedir" ]; then
        rm -rf "$homedir" 2>/dev/null || true
    fi
done < /etc/passwd
echo "2. Other accounts (excluding root and admin) processed!"

echo "3. Removing directories ending with .DEL in home folders..."
# Scan /etc/passwd again and clean *.DEL directories in valid home paths
while IFS=: read -r username _ _ _ _ homedir user_shell; do
    case "$user_shell" in
    */bash|*/sh|*/zsh) ;;
    *) continue ;;
    esac
    if [ -d "$homedir" ]; then
        find "$homedir" -maxdepth 1 -type d -name "*.DEL" -exec rm -rf {} \;
    fi
done < /etc/passwd
echo "3. Cleanup of *.DEL directories completed!"
'

# =============================
# 7. Clear Bash History
# =============================
add_run_cmd '
# history is a bash builtin; the guest /bin/sh (dash on Ubuntu) does not have
# it, so a failure here is expected and ignored.
history -c 2>/dev/null || true
history -w 2>/dev/null || true
'

# =============================
# 8. (Partial Only) Inject SSH Public Key
# =============================
if [[ "$MODE" == "partial" && -n "$SSH_KEY" ]]; then
    log::info "Injecting SSH public key into root account..."
    VIRT_CMD+=(--ssh-inject "root:file:$SSH_KEY")
fi

# =============================
# Execute All Commands (Single Guest Boot)
# =============================
log::highlight "Using virt-customize in single-launch mode for efficiency"
"${VIRT_CMD[@]}"

log::success "Hardening completed successfully!"
