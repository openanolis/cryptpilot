#!/bin/bash

# Unified test script for system disk encryption with cryptpilot
# This script can be used in different modes:
# 1. CI mode - Automated testing with verification
# 2. Local mode - Interactive testing
# 3. Download only mode - Just download images
# 4. Encrypt only mode - Just encrypt an existing image

set -e # Exit on any error

# Default configuration
IMAGE_URL="https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20250117.qcow2"
IMAGE_NAME="alinux3.qcow2"
ENCRYPTED_IMAGE_NAME="encrypted.qcow2"
CONFIG_DIR="test-config"
PASSPHRASE="AAAaaawewe222"
LOG_FILE="qemu-output.log"

# Mode flags
CI_MODE=false
LOCAL_MODE=false
DOWNLOAD_ONLY=false
ENCRYPT_ONLY=false
KEEP_FILES=false

# Package list for cryptpilot-convert
PACKAGES=()

# Paths to existing images (prefixed with @ to indicate they should be used as-is)
EXISTING_IMAGE_PATH=""

# QEMU command to use
QEMU_CMD=""

# Common QEMU paths to check
QEMU_PATHS=(
    "qemu-system-x86_64"
    "qemu-kvm"
    "qemu-system-i386"
    "/usr/libexec/qemu-kvm"
    "/usr/bin/qemu-system-x86_64"
    "/usr/bin/qemu-kvm"
    "/usr/local/bin/qemu-system-x86_64"
)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Help message
show_help() {
    cat <<EOF
Usage: $0 [OPTIONS]

Unified test script for cryptpilot system disk encryption.

OPTIONS:
    -h, --help              Show this help message
    -c, --ci                Run in CI mode (automated testing with verification)
    -l, --local             Run in local mode (interactive testing)
    -d, --download-only     Download images only
    -e, --encrypt-only      Encrypt existing image only
    -k, --keep-files        Keep downloaded files after execution
    --image-url URL         Specify custom image URL
    --image-name NAME       Specify custom image filename
    --encrypted-name NAME   Specify custom encrypted image filename
    --config-dir DIR        Specify custom config directory
    --passphrase PHRASE     Specify custom passphrase
    --package PACKAGE       Specify an RPM package name or path to install (can be used multiple times)
    --existing-image PATH   Use existing image file (prefix with @, e.g., @/path/to/image.qcow2)

EXAMPLES:
    $0 --ci                 # Run full CI test
    $0 --local              # Run local interactive test
    $0 --download-only      # Download images only
    $0 --encrypt-only       # Encrypt existing image
    $0 --ci --keep-files    # Run CI test but keep files
    $0 --ci --package /path/to/package.rpm --package another-package
    $0 --ci --existing-image @/root/diskenc/aliyun_3_x64_20G_nocloud_alibase_20250117.qcow2

EOF
}

log() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Parse command line arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
        -h | --help)
            show_help
            exit 0
            ;;
        -c | --ci)
            CI_MODE=true
            shift
            ;;
        -l | --local)
            LOCAL_MODE=true
            shift
            ;;
        -d | --download-only)
            DOWNLOAD_ONLY=true
            shift
            ;;
        -e | --encrypt-only)
            ENCRYPT_ONLY=true
            shift
            ;;
        -k | --keep-files)
            KEEP_FILES=true
            shift
            ;;
        --image-url)
            IMAGE_URL="$2"
            shift 2
            ;;
        --image-name)
            IMAGE_NAME="$2"
            shift 2
            ;;
        --encrypted-name)
            ENCRYPTED_IMAGE_NAME="$2"
            shift 2
            ;;
        --config-dir)
            CONFIG_DIR="$2"
            shift 2
            ;;
        --passphrase)
            PASSPHRASE="$2"
            shift 2
            ;;
        --package)
            PACKAGES+=("$2")
            shift 2
            ;;
        --existing-image)
            EXISTING_IMAGE_PATH="$2"
            shift 2
            ;;
        *)
            error "Unknown option: $1"
            ;;
        esac
    done
}

# Find appropriate QEMU command
find_qemu() {
    log "Detecting QEMU installation..."

    # Try different QEMU commands and paths
    for path in "${QEMU_PATHS[@]}"; do
        if [[ -x "$path" ]] || command -v "$path" >/dev/null 2>&1; then
            QEMU_CMD="$path"
            log "Found QEMU command: $QEMU_CMD"
            return 0
        fi
    done

    error "No QEMU command found. Please install QEMU (qemu-system-x86 or qemu-kvm)"
}

# Cleanup function
cleanup() {
    if [[ "$KEEP_FILES" == false ]]; then
        log "Cleaning up..."
        [[ -d "${CONFIG_DIR}" ]] && rm -rf "${CONFIG_DIR}"
        # Only clean up downloaded/created files, not existing ones
        if [[ -z "$EXISTING_IMAGE_PATH" ]]; then
            [[ "$DOWNLOAD_ONLY" == false && "$ENCRYPT_ONLY" == false ]] && [[ -f "${IMAGE_NAME}" ]] && rm -f "${IMAGE_NAME}"
        fi
        [[ "$DOWNLOAD_ONLY" == false ]] && [[ -f "${ENCRYPTED_IMAGE_NAME}" ]] && rm -f "${ENCRYPTED_IMAGE_NAME}"
        [[ -S "qemu-monitor.sock" ]] && rm -f "qemu-monitor.sock"
        [[ -f "${LOG_FILE}" ]] && rm -f "${LOG_FILE}"
    else
        log "Keeping downloaded files as requested"
    fi

    # Kill QEMU process if still running
    if [[ -n "${qemu_pid:-}" ]]; then
        log "Terminating QEMU process ${qemu_pid}"
        kill "${qemu_pid}" 2>/dev/null || true
        # Wait a moment for graceful termination
        sleep 2
        # Force kill if still running
        kill -9 "${qemu_pid}" 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Setup config directory
setup_config() {
    log "Setting up cryptpilot config..."
    mkdir -p "${CONFIG_DIR}"

    cat >"${CONFIG_DIR}/fde.toml" <<EOF
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "${PASSPHRASE}"]

[data]
integrity = true

[data.encrypt.exec]
command = "echo"
args = ["-n", "${PASSPHRASE}"]
EOF
}

# Download Alinux3 image with resume capability
download_image() {
    # If existing image path is provided, use it
    if [[ -n "$EXISTING_IMAGE_PATH" ]]; then
        # Remove the @ prefix
        local existing_path="${EXISTING_IMAGE_PATH#@}"
        if [[ -f "$existing_path" ]]; then
            log "Using existing image file: $existing_path"
            # Create a symlink or copy to our expected name
            ln -sf "$existing_path" "${IMAGE_NAME}"
            return
        else
            error "Specified existing image file not found: $existing_path"
        fi
    fi

    if [[ -f "${IMAGE_NAME}" ]]; then
        log "Using existing image file"
        return
    fi

    log "Downloading Alinux3 image from ${IMAGE_URL}..."
    log "NOTE: This is a large file (20GB) and may take a while to download"

    # Try with wget first (supports resume)
    if command -v wget >/dev/null 2>&1; then
        log "Using wget with resume capability"
        wget -c -O "${IMAGE_NAME}" "${IMAGE_URL}" || error "Failed to download image with wget"
    # Try with curl if wget is not available
    elif command -v curl >/dev/null 2>&1; then
        log "Using curl"
        curl -L -o "${IMAGE_NAME}" "${IMAGE_URL}" || error "Failed to download image with curl"
    else
        error "Neither wget nor curl found"
    fi

    [[ -f "${IMAGE_NAME}" ]] || error "Image file not found after download"
    log "Image downloaded successfully"
}

# Find cryptpilot-convert command
find_cryptpilot_convert() {
    log "Detecting cryptpilot-convert installation..."

    # Check if cryptpilot-convert is available in PATH
    if command -v cryptpilot-convert >/dev/null 2>&1; then
        CRYPTPILOT_CONVERT_CMD="cryptpilot-convert"
        log "Found cryptpilot-convert command: $CRYPTPILOT_CONVERT_CMD"
        return 0
    fi

    # Try to use built version
    if [[ -f "target/release/cryptpilot-convert" ]]; then
        CRYPTPILOT_CONVERT_CMD="./target/release/cryptpilot-convert"
        log "Found built cryptpilot-convert: $CRYPTPILOT_CONVERT_CMD"
        return 0
    fi

    # Try to use script version
    if [[ -f "cryptpilot-convert.sh" ]]; then
        chmod +x cryptpilot-convert.sh
        CRYPTPILOT_CONVERT_CMD="./cryptpilot-convert.sh"
        log "Found cryptpilot-convert script: $CRYPTPILOT_CONVERT_CMD"
        return 0
    fi

    error "cryptpilot-convert not found"
}

# Encrypt image with cryptpilot-convert
encrypt_image() {
    if [[ -f "${ENCRYPTED_IMAGE_NAME}" ]]; then
        log "Using existing encrypted image file"
        return
    fi

    log "Encrypting image with cryptpilot-convert..."

    # Find cryptpilot-convert command
    find_cryptpilot_convert

    # Build package arguments
    local package_args=()
    for package in "${PACKAGES[@]}"; do
        package_args+=(--package "$package")
    done

    # Run cryptpilot-convert
    "$CRYPTPILOT_CONVERT_CMD" --in "${IMAGE_NAME}" --out "${ENCRYPTED_IMAGE_NAME}" \
        --config-dir "${CONFIG_DIR}" --rootfs-passphrase "${PASSPHRASE}" \
        "${package_args[@]}" || error "Encryption failed"

    [[ -f "${ENCRYPTED_IMAGE_NAME}" ]] || error "Encrypted image not found after encryption process"
    log "Image encrypted successfully"
}

# Start QEMU with the encrypted image (CI mode)
start_qemu_ci() {
    log "Starting QEMU with encrypted image (CI mode)..."

    # Find appropriate QEMU command
    find_qemu

    # Print QEMU command for debugging
    log "QEMU command: $QEMU_CMD -m 2048M -smp 2 -nographic -serial mon:stdio -monitor unix:qemu-monitor.sock,server,nowait -drive file=${ENCRYPTED_IMAGE_NAME},format=qcow2,if=virtio,id=hd0,readonly=off -netdev user,id=net0,net=192.168.123.0/24,hostfwd=tcp::2222-:22 -device virtio-net,netdev=net0"

    # Start QEMU in background, redirecting output to file for analysis
    $QEMU_CMD \
        -m 2048M \
        -smp 2 \
        -nographic \
        -serial mon:stdio \
        -monitor unix:qemu-monitor.sock,server,nowait \
        -drive file="${ENCRYPTED_IMAGE_NAME}",format=qcow2,if=virtio,id=hd0,readonly=off \
        -netdev user,id=net0,net=192.168.123.0/24,hostfwd=tcp::2222-:22 \
        -device virtio-net,netdev=net0 \
        >"${LOG_FILE}" 2>&1 &
    qemu_pid=$!

    log "QEMU started with PID: ${qemu_pid}"

    # Give QEMU some time to start
    sleep 10

    # Check if QEMU is still running
    if ! kill -0 ${qemu_pid} 2>/dev/null; then
        error "QEMU process terminated unexpectedly"
    fi
}

# Verify system boot by checking for login prompt in output (CI mode)
verify_boot() {
    log "Verifying system boot by checking for login prompt..."

    # Wait for system to boot and show login prompt (max 300 seconds)
    local timeout=300
    local count=0

    while [[ $count -lt $timeout ]]; do
        # Check if log file exists and has content
        if [[ -f "${LOG_FILE}" ]] && [[ -s "${LOG_FILE}" ]]; then
            log "Log file exists and has content, checking for login prompt..."

            # Check if login prompt appears in output log
            if grep -q "login:" "${LOG_FILE}" 2>/dev/null; then
                log "System boot verified - login prompt detected!"
                log "Last 20 lines of output:"
                tail -20 "${LOG_FILE}"
                return 0
            fi

            # Also check for Alibaba Cloud Linux login prompt
            if grep -q "Alibaba Cloud Linux" "${LOG_FILE}" 2>/dev/null; then
                log "System boot verified - Alibaba Cloud Linux detected!"
                log "Last 20 lines of output:"
                tail -20 "${LOG_FILE}"
                return 0
            fi

            # Show progress every 30 seconds
            if ((count % 30 == 0)); then
                log "Still waiting for boot (elapsed: ${count}s)..."
                log "Last 10 lines of log:"
                tail -10 "${LOG_FILE}"
            fi
        elif [[ -f "${LOG_FILE}" ]]; then
            log "Log file exists but is empty (elapsed: ${count}s)"
        else
            log "Log file does not exist yet (elapsed: ${count}s)"
        fi

        # Check if QEMU process is still running
        if ! kill -0 ${qemu_pid} 2>/dev/null; then
            error "QEMU process terminated unexpectedly. Check ${LOG_FILE} for details."
        fi

        sleep 1
        ((count++))
    done

    # If we get here, the system didn't boot in time
    log "System failed to boot within ${timeout} seconds."
    if [[ -f "${LOG_FILE}" ]]; then
        log "Showing last 50 lines of QEMU output:"
        tail -50 "${LOG_FILE}"
    else
        log "No QEMU output log found."
    fi

    error "System failed to boot and show login prompt within ${timeout} seconds."
}

# Check mount entries in /etc/mtab and device mapper volumes
check_mount_entries() {
    log "Checking mount entries and device mapper volumes..."

    # Wait for system to be fully ready (add a small delay)
    sleep 5

    # Try to login via console to verify system is working
    log "Attempting to login via console with username 'root' and password 'root'..."
    # Send username and password via QEMU monitor using sendkey command
    for char in r o o t; do
        echo -e "sendkey ${char}\n" | timeout 10 socat - UNIX-CONNECT:qemu-monitor.sock >/dev/null 2>&1
        sleep 0.1
    done
    echo -e "sendkey ret\n" | timeout 10 socat - UNIX-CONNECT:qemu-monitor.sock >/dev/null 2>&1

    sleep 1

    for char in r o o t; do
        echo -e "sendkey ${char}\n" | timeout 10 socat - UNIX-CONNECT:qemu-monitor.sock >/dev/null 2>&1
        sleep 0.1
    done
    echo -e "sendkey ret\n" | timeout 10 socat - UNIX-CONNECT:qemu-monitor.sock >/dev/null 2>&1

    sleep 3

    # Try to extract information from QEMU guest
    if [[ -S "qemu-monitor.sock" ]]; then
        log "Extracting information from QEMU guest..."

        # Extract /etc/mtab from guest
        echo -e "human-monitor-command {\"command-line\":\"cat /etc/mtab\"}\nquit" | socat - UNIX-CONNECT:qemu-monitor.sock >mtab_output.txt 2>/dev/null

        # Extract /dev/mapper contents from guest
        echo -e "human-monitor-command {\"command-line\":\"ls -1 /dev/mapper\"}\nquit" | socat - UNIX-CONNECT:qemu-monitor.sock >dev_mapper_output.txt 2>/dev/null

        # Check for rootfs mount entry (only checking the first 3 columns)
        if grep -q "^/dev/mapper/rootfs / overlay" mtab_output.txt; then
            log "Rootfs mount entry found in /etc/mtab!"
        else
            log "Rootfs mount entry NOT found in /etc/mtab."
            log "Current mtab entries:"
            cat mtab_output.txt
            return 1
        fi

        # Check for data mount entry
        if grep -q "^/dev/mapper/data /data " mtab_output.txt; then
            log "Data mount entry found in /etc/mtab!"
        else
            log "/data mount entry NOT found in /etc/mtab."
            log "Current mtab entries:"
            cat mtab_output.txt
            return 1
        fi

        # Check for rootfs device mapper volume
        if grep -q "^rootfs$" dev_mapper_output.txt; then
            log "Rootfs device mapper volume found!"
        else
            log "Rootfs device mapper volume NOT found."
            log "Current /dev/mapper contents:"
            cat dev_mapper_output.txt
            return 1
        fi

        # Check for data device mapper volume
        if grep -q "^data$" dev_mapper_output.txt; then
            log "Data device mapper volume found!"
        else
            log "Data device mapper volume NOT found."
            log "Current /dev/mapper contents:"
            cat dev_mapper_output.txt
            return 1
        fi

        log "All mount entries and device mapper volumes verified successfully!"
        return 0
    else
        warn "QEMU monitor socket not available, skipping mount and device mapper checks"
        return 0
    fi
}

# Test container functionality by installing podman and running a simple container
test_container_functionality() {
    log "Testing container functionality..."

    if [[ -S "qemu-monitor.sock" ]]; then
        log "Installing podman and running test container..."
        
        # Install podman
        echo -e "human-monitor-command {\"command-line\":\"yum install -y podman\"}\nquit" | socat - UNIX-CONNECT:qemu-monitor.sock >podman_install_output.txt 2>/dev/null
        sleep 10
        
        # Run test container with echo command
        echo -e "human-monitor-command {\"command-line\":\"podman run --rm ghcr.io/linuxcontainers/alpine:latest echo 'Hello from container!'\"}\nquit" | socat - UNIX-CONNECT:qemu-monitor.sock >container_test_output.txt 2>/dev/null
        sleep 5
        
        # Check if the container ran successfully
        if grep -q "Hello from container!" container_test_output.txt; then
            log "Container test passed successfully!"
            return 0
        else
            log "Container test failed."
            log "Container test output:"
            cat container_test_output.txt
            return 1
        fi
    else
        warn "QEMU monitor socket not available, skipping container functionality test"
        return 0
    fi
}

# Start QEMU with the encrypted image (Local mode)
start_qemu_local() {
    log "Starting QEMU with encrypted image (Local mode)..."

    # Find appropriate QEMU command
    find_qemu

    # Start QEMU with a simplified configuration for testing
    log "Starting QEMU - Press Ctrl+A then X to exit"
    $QEMU_CMD \
        -m 2048M \
        -smp 2 \
        -nographic \
        -serial mon:stdio \
        -drive file="${ENCRYPTED_IMAGE_NAME}",format=qcow2,if=virtio,id=hd0,readonly=off
}

# Main execution
main() {
    if [ "$(id -u)" != "0" ]; then
        log::error "This script must be run as root"
        exit 1
    fi

    parse_args "$@"

    # Determine mode if not explicitly set
    if [[ "$CI_MODE" == false && "$LOCAL_MODE" == false && "$DOWNLOAD_ONLY" == false && "$ENCRYPT_ONLY" == false ]]; then
        # Default to CI mode
        CI_MODE=true
    fi

    log "Starting unified test script for system disk encryption"

    # Handle download-only mode
    if [[ "$DOWNLOAD_ONLY" == true ]]; then
        download_image
        log "Download-only mode completed successfully!"
        exit 0
    fi

    # Handle encrypt-only mode
    if [[ "$ENCRYPT_ONLY" == true ]]; then
        setup_config
        encrypt_image
        log "Encrypt-only mode completed successfully!"
        exit 0
    fi

    # Setup config for other modes
    setup_config

    # Download images or use existing ones
    download_image
    
    # 使用virt-customize设置root密码
    set_root_password

    # Encrypt image
    encrypt_image

    # Handle CI mode
    if [[ "$CI_MODE" == true ]]; then
        start_qemu_ci
        verify_boot
        check_mount_entries
        test_container_functionality
        log "CI mode test completed successfully!"
        log "System disk encryption verification passed!"
    fi

    # Handle local mode
    if [[ "$LOCAL_MODE" == true ]]; then
        start_qemu_local
        log "Local mode test completed!"
    fi
}

# Set root password using virt-customize
set_root_password() {
    log "Setting root password using virt-customize..."
    
    # Check if virt-customize is available
    if ! command -v virt-customize >/dev/null 2>&1; then
        error "virt-customize not found. Please install libguestfs-tools package."
    fi
    
    # Use virt-customize to set root password to root
    virt-customize -a "${IMAGE_NAME}" --root-password password:root || error "Failed to set root password"
    
    log "Root password set successfully"
}

# Run main function
main "$@"
