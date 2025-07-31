#!/bin/bash

# Test script for KBS volume encryption with cryptpilot
# This script tests the KBS credential provider for data volume encryption

set -e  # Exit on any error

# Default configuration
CONFIG_DIR="test-kbs-config"
LOG_FILE="test-output.log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Help message
show_help() {
    cat << EOF
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
            -h|--help)
                show_help
                exit 0
                ;;
            -k|--keep-files)
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
        [[ -f "${LOG_FILE}" ]] && rm -f "${LOG_FILE}"
        
        # Detach any loop device associated with test-disk.img
        if sudo losetup -a | grep -q "test-disk.img"; then
            LOOP_DEV=$(sudo losetup -a | grep "test-disk.img" | cut -d: -f1)
            sudo losetup -d $LOOP_DEV 2>/dev/null || true
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
    cat > "${CONFIG_DIR}/volumes/kbs-test.toml" << EOF
volume = "kbs-test"
dev = "/dev/loop99p1"
auto_open = true
makefs = "ext4"
integrity = true

# For CI test, we use exec provider to simulate KBS behavior
# In real usage, this would be [encrypt.kbs] with actual KBS configuration
[encrypt.exec]
command = "echo"
args = ["-n", "test-passphrase-for-kbs"]
EOF
}

# Create test disk
create_test_disk() {
    log "Creating test disk..."
    
    # Create a 100MB disk image
    dd if=/dev/zero of=test-disk.img bs=1M count=100
    
    # Detach loop device if already attached
    for i in {0..9}; do
        if sudo losetup -a | grep -q "test-disk.img"; then
            LOOP_DEV=$(sudo losetup -a | grep "test-disk.img" | cut -d: -f1)
            sudo losetup -d $LOOP_DEV 2>/dev/null || true
        fi
    done
    
    # Set up loop device (try different numbers if /dev/loop99 is busy)
    LOOP_DEV=""
    for i in {99..90}; do
        if ! sudo losetup -P /dev/loop$i test-disk.img 2>/dev/null; then
            continue
        else
            LOOP_DEV="/dev/loop$i"
            break
        fi
    done
    
    if [[ -z "$LOOP_DEV" ]]; then
        error "Failed to set up loop device"
    fi
    
    # Update the config file to use the actual loop device
    sed -i "s|/dev/loop99|${LOOP_DEV}|g" ${CONFIG_DIR}/volumes/kbs-test.toml
    
    # Create GPT partition table and one partition
    sudo parted --script $LOOP_DEV \
        mktable gpt \
        mkpart primary 1MiB 100%
    
    # Wait for partition to be ready
    sleep 2
    
    log "Test disk created: $LOOP_DEV"
}

# Test volume initialization
test_volume_init() {
    log "Initializing KBS volume..."
    
    # Initialize the volume
    if ! sudo cryptpilot init kbs-test -c "${CONFIG_DIR}" -y > "${LOG_FILE}" 2>&1; then
        echo "Failed to initialize volume. Log output:"
        cat "${LOG_FILE}"
        return 1
    fi
    
    log "Volume initialized successfully"
}

# Test volume opening
test_volume_open() {
    log "Opening KBS volume..."
    
    # Open the volume
    if ! sudo cryptpilot open kbs-test -c "${CONFIG_DIR}" > "${LOG_FILE}" 2>&1; then
        echo "Failed to open volume. Log output:"
        cat "${LOG_FILE}"
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
    if ! sudo cryptpilot show -c "${CONFIG_DIR}" > "${LOG_FILE}" 2>&1; then
        echo "Failed to show volume status. Log output:"
        cat "${LOG_FILE}"
        return 1
    fi
    
    # Check if our volume is listed and opened
    if ! grep -q "kbs-test" "${LOG_FILE}"; then
        echo "Volume kbs-test not found in show output"
        return 1
    fi
    
    if ! grep -q "True" "${LOG_FILE}"; then
        echo "Volume kbs-test is not opened according to show output"
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
    if ! sudo mount /dev/mapper/kbs-test /tmp/kbs-test-mount; then
        error "Failed to mount volume"
    fi
    
    # Test writing and reading a file
    echo "test content" | sudo tee /tmp/kbs-test-mount/test-file > /dev/null
    if [[ ! -f "/tmp/kbs-test-mount/test-file" ]]; then
        error "Failed to create test file"
    fi
    
    content=$(sudo cat /tmp/kbs-test-mount/test-file)
    if [[ "$content" != "test content" ]]; then
        error "File content mismatch"
    fi
    
    # Unmount the volume
    sudo umount /tmp/kbs-test-mount
    
    log "Filesystem operations tested successfully"
}

# Test volume closing
test_volume_close() {
    log "Closing KBS volume..."
    
    # Close the volume
    if ! sudo cryptpilot close kbs-test -c "${CONFIG_DIR}" > "${LOG_FILE}" 2>&1; then
        echo "Failed to close volume. Log output:"
        cat "${LOG_FILE}"
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
    parse_args "$@"
    
    log "Starting KBS volume encryption test"
    
    # Setup config
    setup_config
    
    # Create test disk
    create_test_disk
    
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