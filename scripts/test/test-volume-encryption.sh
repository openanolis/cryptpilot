#!/bin/bash

# Test script for KBS volume encryption with cryptpilot
# This script tests the KBS credential provider for data volume encryption

set -e # Exit on any error

TRUSTEE_URL="http://127.0.0.1:8081/api/" # Trustee service URL

# Function to check and install required commands
check_and_install_commands() {
    local missing_commands=()
    local install_commands=()

    # List of required commands and their packages (for RHEL/CentOS)
    local commands_and_packages=(
        "losetup:util-linux"
        "mount:util-linux"
        "umount:util-linux"
    )

    # Check which commands are missing
    for item in "${commands_and_packages[@]}"; do
        local cmd="${item%%:*}"
        local pkg="${item#*:}"

        if ! command -v "$cmd" >/dev/null 2>&1; then
            missing_commands+=("$cmd")
            install_commands+=("$pkg")
        fi
    done

    # If there are missing commands, try to install them
    if [ ${#missing_commands[@]} -gt 0 ]; then
        echo "Missing required commands: ${missing_commands[*]}"

        echo "Attempting to install missing packages with yum..."
        if ! yum install -y "${install_commands[@]}"; then
            echo "Failed to install packages with yum. Please install the following packages manually:"
            echo "${install_commands[*]}"
            exit 1
        fi
    fi

    echo "All required commands are available."
}

# Default configuration
CONFIG_DIR="test-kbs-config"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Help message
show_help() {
    cat <<EOF
Usage: $0 [OPTIONS]

Test script for cryptpilot KBS volume encryption.

OPTIONS:
    -h, --help              Show this help message
    -k, --keep-files        Keep test files after execution

EXAMPLES:
    $0                  # Run KBS volume test
    $0 --keep-files     # Run test but keep files

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
KEEP_FILES=false

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
        -h | --help)
            show_help
            exit 0
            ;;
        -k | --keep-files)
            KEEP_FILES=true
            shift
            ;;
        *)
            error "Unknown option: $1"
            ;;
        esac
    done
}

# Cleanup function
cleanup() {
    if [[ "$KEEP_FILES" == false ]]; then
        log "Cleaning up..."
        [[ -d "${CONFIG_DIR}" ]] && rm -rf "${CONFIG_DIR}"

        # Detach any loop device associated with test-disk.img
        if losetup -a | grep -q "test-disk.img"; then
            LOOP_DEV=$(losetup -a | grep "test-disk.img" | cut -d: -f1)
            losetup -d "$LOOP_DEV" 2>/dev/null || true
        fi

        [[ -f "test-disk.img" ]] && rm -f "test-disk.img"
        [[ -d "/tmp/kbs-test-mount" ]] && rmdir /tmp/kbs-test-mount 2>/dev/null || true
    else
        log "Keeping test files as requested"
    fi
}

trap cleanup EXIT

# Setup config directory for KBS volume
setup_config() {
    log "Setting up cryptpilot config for KBS volume..."
    mkdir -p "${CONFIG_DIR}/volumes"

    # Create a KBS volume config (using mock values for CI)
    cat >"${CONFIG_DIR}/volumes/kbs-test.toml" <<EOF
volume = "kbs-test"
dev = "${LOOP_DEV}"
auto_open = true
makefs = "ext4"
integrity = true

[encrypt.kbs]
kbs_url = "${TRUSTEE_URL}"
key_uri = "kbs:///default/local-resources/volume"
EOF
}

# Create test disk
create_test_disk() {
    log "Creating test disk..."

    # Create a 5GB disk image
    dd if=/dev/zero of=test-disk.img bs=5M count=1024

    # Detach loop device if already attached
    LOOP_DEVICE=$(losetup --associated "$IMAGE" | awk '{print $1}')
    if [ -n "$LOOP_DEVICE" ]; then
        losetup -d "$LOOP_DEV" 2>/dev/null || true
    fi

    # Set up loop device (try different numbers if /dev/loop99 is busy)
    LOOP_DEV=$(losetup --find)

    if [[ -z "$LOOP_DEV" ]]; then
        error "Failed to found a free loop device"
    fi

    if ! losetup -P "$LOOP_DEV" test-disk.img 2>/dev/null; then
        error "Failed to set up loop device"
    fi

    # Wait for partition to be ready
    sleep 2

    log "Test disk created: $LOOP_DEV"
}

# Test volume initialization
test_volume_init() {
    log "Initializing KBS volume..."

    # check if trustee is still running
    yum install -y iproute
    ps -ef

    ss --tcp -n --listening

    # Initialize the volume
    if ! cryptpilot init kbs-test -c "${CONFIG_DIR}" -y; then
        echo "Failed to initialize volume."

        cat /tmp/trustee-gateway.log

        # check again trustee is still running
        ps -ef

        ss --tcp -n --listening

        return 1
    fi

    log "Volume initialized successfully"
}

# Test volume opening
test_volume_open() {
    log "Opening KBS volume..."

    # Open the volume
    if ! cryptpilot open kbs-test -c "${CONFIG_DIR}"; then
        echo "Failed to open volume."
        return 1
    fi

    # Check if the device mapper device exists
    if [[ ! -b "/dev/mapper/kbs-test" ]]; then
        echo "Device mapper device /dev/mapper/kbs-test not found"
        return 1
    fi

    log "Volume opened successfully"
}

# Test volume show
test_volume_show() {
    log "Showing volume status..."

    # Show volume status
    if ! cryptpilot show -c "${CONFIG_DIR}"; then
        echo "Failed to show volume status."
        return 1
    fi

    log "Volume status verified successfully"
}

# Test filesystem operations
test_filesystem_ops() {
    log "Testing filesystem operations..."

    # Create mount point
    mkdir -p /tmp/kbs-test-mount

    # Mount the volume
    if ! mount /dev/mapper/kbs-test /tmp/kbs-test-mount; then
        error "Failed to mount volume"
    fi

    # Test writing and reading a file
    echo "test content" | tee /tmp/kbs-test-mount/test-file >/dev/null
    if [[ ! -f "/tmp/kbs-test-mount/test-file" ]]; then
        error "Failed to create test file"
    fi

    content=$(cat /tmp/kbs-test-mount/test-file)
    if [[ "$content" != "test content" ]]; then
        error "File content mismatch"
    fi

    # Unmount the volume
    umount /tmp/kbs-test-mount

    log "Filesystem operations tested successfully"
}

# Test volume closing
test_volume_close() {
    log "Closing KBS volume..."

    # Close the volume
    if ! cryptpilot close kbs-test -c "${CONFIG_DIR}"; then
        echo "Failed to close volume."
        return 1
    fi

    # Check that the device mapper device no longer exists
    if [[ -b "/dev/mapper/kbs-test" ]]; then
        echo "Device mapper device /dev/mapper/kbs-test still exists after closing"
        return 1
    fi

    log "Volume closed successfully"
}

# Main execution
main() {
    if [ "$(id -u)" != "0" ]; then
        log::error "This script must be run as root"
        exit 1
    fi

    parse_args "$@"

    # Check and install required commands
    check_and_install_commands

    log "Starting KBS volume encryption test"

    # Create test disk
    create_test_disk

    # Setup config
    setup_config

    # Test volume operations
    if ! test_volume_init; then
        error "Volume initialization failed"
    fi

    if ! test_volume_open; then
        error "Volume opening failed"
    fi

    if ! test_volume_show; then
        error "Volume show failed"
    fi

    if ! test_filesystem_ops; then
        error "Filesystem operations failed"
    fi

    if ! test_volume_close; then
        error "Volume closing failed"
    fi

    log "KBS volume encryption test completed successfully!"
}

# Run main function
main "$@"
