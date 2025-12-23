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
    usage
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

log::info "Starting image hardening..."
log::highlight "Image: $IMAGE"
log::highlight "Mode: $MODE"

# Build virt-customize command (single execution)
VIRT_CMD=(
    virt-customize
    --format=qcow2
    -a "$IMAGE"
)

# Helper function to append --run-command
add_run_cmd() {
    VIRT_CMD+=(--run-command "$1")
}

# =============================
# 1. Uninstall Cloud Assistant Agent (Aliyun Assist)
# =============================
add_run_cmd '
# Stop Cloud Assistant daemon
/usr/local/share/assist-daemon/assist_daemon --stop

# Stop Cloud Assistant service
systemctl stop aliyun.service

# Remove Cloud Assistant daemon
/usr/local/share/assist-daemon/assist_daemon --delete

# Uninstall package
rpm -qa | grep aliyun_assist | xargs rpm -e

# Clean up leftover files and service configurations
rm -rf /usr/local/share/aliyun-assist
rm -rf /usr/local/share/assist-daemon
rm -f /etc/systemd/system/aliyun.service
rm -f /etc/init.d/aliyun-service
'

# =============================
# 2. Uninstall Cloud Security Center (Aegis/AntiKnight)
# =============================
add_run_cmd '
# Download and execute uninstall script
wget "http://update.aegis.aliyun.com/download/uninstall.sh" && chmod +x uninstall.sh && ./uninstall.sh
'

# =============================
# 3. Disable and Mask rpcbind Service
# =============================
add_run_cmd '
systemctl stop rpcbind.service
systemctl disable rpcbind.service
systemctl mask rpcbind.service

systemctl stop rpcbind.socket
systemctl disable rpcbind.socket
systemctl mask rpcbind.socket
'

# =============================
# 4. Remove cloud-init
# =============================
add_run_cmd '
yum remove -y cloud-init
'

# =============================
# 5. Optional: Restrict or Remove SSH Service
# =============================
if [[ "$MODE" == "full" ]]; then
    add_run_cmd '
    yum remove -y openssh-server
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
#!/bin/bash

# 1. Lock root and admin account passwords:
echo "1. Locking root and admin account passwords..."
sed -i "s/\(^root:\)[^:]*/\1!!/" /etc/shadow
sed -i "s/\(^admin:\)[^:]*/\1!!/" /etc/shadow
echo "1. Root and admin account passwords locked!"

echo "2. Processing other user accounts (excluding root and admin)..."
# Define list of excluded usernames
exclude_users=("root" "admin")

while IFS=: read -r username _ _ _ _ homedir user_shell; do
  # Check if shell is one of the interactive shells
  case $user_shell in
    "/bin/bash"|"/bin/sh"|"/bin/zsh" \
    |"/usr/bin/bash"|"/usr/bin/sh"|"/usr/bin/zsh" \
    |"/usr/local/bin/bash"|"/usr/local/bin/sh"|"/usr/local/bin/zsh")
      
      # Skip if username is in exclude list
      if [[ " ${exclude_users[@]} " =~ " $username " ]]; then
        continue
      fi
      
      # Read password field from shadow
      pass=$(grep "^$username:" /etc/shadow | cut -d: -f2)
      
      # If already locked (!, !!, *, etc.), skip deletion
      if [[ "$pass" == "!" || "$pass" == "!!" || "$pass" == *\!* || "$pass" == "*" ]]; then
        echo "Account $username is already locked or disabled, skipping."
        continue
      fi
      
      # Delete eligible user account and home directory
      echo "Removing account: $username"
      userdel -r "$username" 2>/dev/null || true
      if [[ -d "$homedir" ]]; then
        rm -rf "$homedir" 2>/dev/null || true
      fi
      ;;

    *)
      # Ignore users with non-interactive shells
      continue
      ;;
  esac
done < /etc/passwd
echo "2. Other accounts (excluding root and admin) processed!"

echo "3. Removing directories ending with .DEL in home folders..."
# Scan /etc/passwd again and clean *.DEL directories in valid home paths
while IFS=: read -r username _ _ _ _ homedir user_shell; do
  case $user_shell in
    "/bin/bash"|"/bin/sh"|"/bin/zsh" \
    |"/usr/bin/bash"|"/usr/bin/sh"|"/usr/bin/zsh" \
    |"/usr/local/bin/bash"|"/usr/local/bin/sh"|"/usr/local/bin/zsh")
      if [[ -d "$homedir" ]]; then
        find "$homedir" -maxdepth 1 -type d -name "*.DEL" -exec rm -rf {} \;
      fi
      ;;
  esac
done < /etc/passwd
echo "3. Cleanup of *.DEL directories completed!"
'

# =============================
# 7. Clear Bash History
# =============================
add_run_cmd '
history -c && history -w
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
