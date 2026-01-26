#!/bin/bash
#
# Integration tests for cryptpilot-convert
#
# This script tests the cryptpilot-convert tool's disk conversion capability
# with 4 test combinations:
#   - uki-encrypted:  UKI mode with rootfs encryption
#   - uki-noenc:      UKI mode without rootfs encryption
#   - grub-encrypted: GRUB mode with rootfs encryption
#   - grub-noenc:     GRUB mode without rootfs encryption
#
# Usage:
#   ./tests/test-convert.sh --case <case-name>  # Run specific test case
#   ./tests/test-convert.sh --all               # Run all 4 test cases
#   ./tests/test-convert.sh --help              # Show usage
#

set -e # Exit on error
set -u # Exit on undefined variable
shopt -s nullglob

# Ensure consistent locale for parsing.
export LC_ALL=C

# ANSI color codes
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly CYAN='\033[0;36m'
readonly NC='\033[0m' # No Color

# Test configuration
readonly TEST_IMAGE_URL="https://alinux3.oss-cn-hangzhou.aliyuncs.com/aliyun_3_x64_20G_nocloud_alibase_20251030.qcow2"
readonly TEST_IMAGE_CACHE="/tmp/test-input-alinux3.qcow2"
readonly TEST_PASSPHRASE="test-passphrase-12345"

# Source image path (can be overridden via --input)
SOURCE_IMAGE=""

# Script directory (where this script is located)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Working directory (will be set in main)
WORKDIR=""

# ============================================================================
# Logging functions
# ============================================================================

log::info() {
    printf "${CYAN}[INFO]  %s${NC}\n" "$*" >&2
}

log::success() {
    printf "${GREEN}[PASS]  %s${NC}\n" "$*" >&2
}

log::warn() {
    printf "${YELLOW}[WARN]  %s${NC}\n" "$*" >&2
}

log::error() {
    printf "${RED}[ERROR] %s${NC}\n" "$*" >&2
}

log::step() {
    printf "${GREEN}[STEP]  %s${NC}\n" "$*" >&2
}

fatal() {
    log::error "$@"
    exit 1
}

# ============================================================================
# Utility functions
# ============================================================================

# Check if running as root
check_root() {
    if [[ $EUID -ne 0 ]]; then
        fatal "This script must be run as root"
    fi
}

# Check required tools
check_tools() {
    local tools=("wget" "qemu-img" "qemu-nbd" "cryptsetup" "lvm" "parted" "blkid" "mkfs.ext4" "virt-customize")
    local missing=()

    for tool in "${tools[@]}"; do
        if ! command -v "$tool" &>/dev/null; then
            missing+=("$tool")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        fatal "Missing required tools: ${missing[*]}. Install libguestfs-tools for virt-customize."
    fi
}

# Check available disk space in /tmp
check_disk_space() {
    local required_gb=10
    local available_kb
    available_kb=$(df /tmp | awk 'NR==2 {print $4}')
    local available_gb=$((available_kb / 1024 / 1024))

    if [[ $available_gb -lt $required_gb ]]; then
        fatal "Insufficient disk space in /tmp. Required: ${required_gb}GB, Available: ${available_gb}GB"
    fi
    log::info "Disk space check passed: ${available_gb}GB available in /tmp"
}

# Load nbd kernel module
load_nbd_module() {
    if ! lsmod | grep -q nbd; then
        log::info "Loading nbd kernel module..."
        if ! modprobe nbd max_part=16 2>/dev/null; then
            log::error "Failed to load nbd module"
            log::error "NBD module is required. Ensure nbd is loaded on host system."
            return 1
        fi
    fi
    return 0
}

# Check for conflicting LVM volume group
check_vg_conflict() {
    if [[ -e /dev/system ]] || vgs system &>/dev/null; then
        fatal "LVM volume group 'system' already exists on this host. " \
              "The test cannot run on machines with an existing 'system' VG. " \
              "Please run tests in a container or VM without conflicting VGs."
    fi
}

# Find an available NBD device
get_available_nbd() {
    local nbd
    for nbd in /dev/nbd{0..15}; do
        if [[ -e "$nbd" ]] && [[ $(blockdev --getsize64 "$nbd" 2>/dev/null || echo 0) -eq 0 ]]; then
            echo "$nbd"
            return 0
        fi
    done
    fatal "No available NBD device found"
}

# ============================================================================
# Setup and cleanup functions
# ============================================================================

# Create working directory
setup_workdir() {
    WORKDIR=$(mktemp -d /tmp/cryptpilot-convert-test-XXXXXX)
    log::info "Created working directory: ${WORKDIR}"
}

# Cleanup function - called on exit via trap
# shellcheck disable=SC2329
cleanup() {
    local exit_code=$?
    set +e

    log::info "Cleaning up..."

    # Unmount any mounted filesystems in workdir
    if [[ -n "${WORKDIR:-}" ]] && [[ -d "${WORKDIR}" ]]; then
        for mnt in "${WORKDIR}"/mnt-*; do
            if mountpoint -q "$mnt" 2>/dev/null; then
                log::info "Unmounting $mnt"
                umount -R "$mnt" 2>/dev/null || umount -l "$mnt" 2>/dev/null || true
            fi
        done
    fi

    # Close any LUKS volumes we opened
    for dm in /dev/mapper/test-rootfs-*; do
        if [[ -e "$dm" ]]; then
            log::info "Closing LUKS volume: $dm"
            cryptsetup close "$(basename "$dm")" 2>/dev/null || true
        fi
    done

    # Deactivate LVM volume groups created during tests
    for vg in $(vgs --noheadings -o vg_name 2>/dev/null | grep -E "^[[:space:]]*system" || true); do
        vg=$(echo "$vg" | tr -d ' ')
        log::info "Deactivating VG: $vg"
        vgchange -an "$vg" 2>/dev/null || true
    done

    # Disconnect any NBD devices we connected
    for nbd in /dev/nbd{0..15}; do
        if [[ -e "$nbd" ]] && [[ $(blockdev --getsize64 "$nbd" 2>/dev/null || echo 0) -gt 0 ]]; then
            # Check if this nbd is from our test by looking at connected image path
            if qemu-nbd --disconnect "$nbd" 2>/dev/null; then
                log::info "Disconnected NBD: $nbd"
            fi
        fi
    done

    # Remove working directory
    if [[ -n "${WORKDIR:-}" ]] && [[ -d "${WORKDIR}" ]]; then
        log::info "Removing working directory: ${WORKDIR}"
        rm -rf "${WORKDIR}"
    fi

    if [[ $exit_code -ne 0 ]]; then
        log::error "Test failed with exit code: $exit_code"
    fi

    exit "$exit_code"
}

# ============================================================================
# Test image functions
# ============================================================================

# Download test image with caching
download_test_image() {
    if [[ -f "${TEST_IMAGE_CACHE}" ]]; then
        log::info "Using cached test image: ${TEST_IMAGE_CACHE}"
        return 0
    fi

    log::step "Downloading test image..."
    log::info "URL: ${TEST_IMAGE_URL}"
    log::info "Destination: ${TEST_IMAGE_CACHE}"

    local tmp_file="${TEST_IMAGE_CACHE}.downloading"

    # Download with resume support and retry
    local retry=0
    local max_retries=3
    while [[ $retry -lt $max_retries ]]; do
        if wget -c -O "${tmp_file}" "${TEST_IMAGE_URL}"; then
            mv "${tmp_file}" "${TEST_IMAGE_CACHE}"
            log::success "Test image downloaded successfully"
            return 0
        fi
        retry=$((retry + 1))
        log::warn "Download failed, retry $retry/$max_retries..."
        sleep 5
    done

    rm -f "${tmp_file}"
    fatal "Failed to download test image after $max_retries attempts"
}

# Create test configuration directory with OTP provider
create_test_config() {
    local config_dir="$1"
    mkdir -p "${config_dir}"

    # Create fde.toml with OTP provider (simplest, no external dependencies)
    cat > "${config_dir}/fde.toml" <<'EOF'
# Test configuration for cryptpilot-convert integration tests
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.otp]

[data]
integrity = false

[data.encrypt.otp]
EOF

    log::info "Created test config at: ${config_dir}/fde.toml"
}

# ============================================================================
# Test execution functions
# ============================================================================

# Run cryptpilot-enhance to harden the image before conversion
run_enhance() {
    local test_name="$1"
    local input_image="$2"

    log::step "Running cryptpilot-enhance for test: ${test_name}"

    # Use 'direct' backend to avoid libvirtd dependency in CI/containers
    export LIBGUESTFS_BACKEND=direct

    local cmd=("${REPO_ROOT}/cryptpilot-enhance.sh")
    cmd+=("--mode" "partial")
    cmd+=("--image" "${input_image}")

    log::info "Command: ${cmd[*]}"

    # Run the enhancement
    if ! "${cmd[@]}"; then
        log::error "cryptpilot-enhance failed for test: ${test_name}"
        return 1
    fi

    log::success "cryptpilot-enhance completed for test: ${test_name}"
    return 0
}

# Run cryptpilot-convert with specified parameters
run_convert() {
    local test_name="$1"
    local input_image="$2"
    local output_image="$3"
    local config_dir="$4"
    local use_uki="$5"
    local use_encryption="$6"

    log::step "Running cryptpilot-convert for test: ${test_name}"

    local cmd=("${REPO_ROOT}/cryptpilot-convert.sh")
    cmd+=("--in" "${input_image}")
    cmd+=("--out" "${output_image}")
    cmd+=("--config-dir" "${config_dir}")

    if [[ "${use_uki}" == "true" ]]; then
        cmd+=("--uki")
    fi

    if [[ "${use_encryption}" == "true" ]]; then
        cmd+=("--rootfs-passphrase" "${TEST_PASSPHRASE}")
    else
        cmd+=("--rootfs-no-encryption")
    fi

    log::info "Command: ${cmd[*]}"

    # Run the conversion
    if ! "${cmd[@]}"; then
        log::error "cryptpilot-convert failed for test: ${test_name}"
        return 1
    fi

    log::success "cryptpilot-convert completed for test: ${test_name}"
    return 0
}

# Verify converted image structure
verify_converted_image() {
    local test_name="$1"
    local output_image="$2"
    local use_uki="$3"
    local use_encryption="$4"

    log::step "Verifying converted image for test: ${test_name}"

    # Check output file exists and has non-zero size
    if [[ ! -f "${output_image}" ]]; then
        log::error "Output image does not exist: ${output_image}"
        return 1
    fi

    local file_size
    file_size=$(stat -c%s "${output_image}")
    if [[ $file_size -eq 0 ]]; then
        log::error "Output image is empty: ${output_image}"
        return 1
    fi
    log::info "Output image size: $((file_size / 1024 / 1024 / 1024))GB"

    # Connect image via NBD
    local nbd_device
    nbd_device=$(get_available_nbd)
    log::info "Connecting image to NBD device: ${nbd_device}"

    if ! qemu-nbd --connect="${nbd_device}" "${output_image}"; then
        log::error "Failed to connect image to NBD"
        return 1
    fi

    # Wait for device to be ready
    sleep 2
    partprobe "${nbd_device}" 2>/dev/null || true
    sleep 1

    local verify_failed=0

    # Check partition layout
    log::info "Checking partition layout..."
    if ! lsblk "${nbd_device}"; then
        log::error "Failed to list partitions"
        verify_failed=1
    fi

    # Check for EFI partition
    if ! blkid "${nbd_device}p1" 2>/dev/null | grep -q -i "vfat\|fat"; then
        log::warn "EFI partition (p1) may not be FAT format"
    else
        log::info "EFI partition found"
    fi

    # Check for LVM partition and volume group
    log::info "Scanning for LVM..."
    pvscan --cache 2>/dev/null || true
    vgscan 2>/dev/null || true

    if ! vgs system &>/dev/null; then
        log::error "LVM volume group 'system' not found"
        verify_failed=1
    else
        log::info "LVM volume group 'system' found"

        # Check for logical volumes
        if ! lvs system/rootfs &>/dev/null; then
            log::error "Logical volume 'system/rootfs' not found"
            verify_failed=1
        else
            log::info "Logical volume 'system/rootfs' found"
        fi

        if ! lvs system/rootfs_hash &>/dev/null; then
            log::error "Logical volume 'system/rootfs_hash' not found"
            verify_failed=1
        else
            log::info "Logical volume 'system/rootfs_hash' found"
        fi
    fi

    # Check encryption status
    if [[ "${use_encryption}" == "true" ]]; then
        log::info "Checking LUKS encryption..."
        vgchange -ay system 2>/dev/null || true
        if cryptsetup isLuks /dev/mapper/system-rootfs 2>/dev/null; then
            log::info "LUKS encryption verified on system-rootfs"
        else
            log::error "Expected LUKS encryption on system-rootfs but not found"
            verify_failed=1
        fi
    else
        log::info "Skipping encryption check (no-encryption mode)"
    fi

    # Check UKI vs GRUB specific items
    if [[ "${use_uki}" == "true" ]]; then
        log::info "Checking UKI boot configuration..."
        # Mount EFI partition to check for UKI
        local efi_mount="${WORKDIR}/mnt-efi-${test_name}"
        mkdir -p "${efi_mount}"
        if mount "${nbd_device}p1" "${efi_mount}" 2>/dev/null; then
            if [[ -f "${efi_mount}/EFI/BOOT/BOOTX64.EFI" ]]; then
                log::info "UKI image found at EFI/BOOT/BOOTX64.EFI"
            else
                log::error "UKI image not found at expected location"
                ls -la "${efi_mount}/EFI/" 2>/dev/null || true
                verify_failed=1
            fi
            umount "${efi_mount}" 2>/dev/null || true
        else
            log::warn "Could not mount EFI partition to verify UKI"
        fi
    else
        log::info "Checking GRUB boot configuration..."
        # In GRUB mode, there should be a boot partition
        # The boot partition could be p2 or embedded in rootfs depending on layout
        log::info "GRUB mode verification: checking for boot-related partitions"
        lsblk -o NAME,SIZE,TYPE,FSTYPE "${nbd_device}" || true
    fi

    # Cleanup: deactivate VG and disconnect NBD
    log::info "Cleaning up verification mounts..."
    vgchange -an system 2>/dev/null || true
    sleep 1
    qemu-nbd --disconnect "${nbd_device}" 2>/dev/null || true

    if [[ $verify_failed -eq 0 ]]; then
        log::success "Verification passed for test: ${test_name}"
        return 0
    else
        log::error "Verification failed for test: ${test_name}"
        return 1
    fi
}

# ============================================================================
# Test case functions
# ============================================================================

run_test_case() {
    local test_name="$1"
    local use_uki="$2"
    local use_encryption="$3"

    log::step "=========================================="
    log::step "Running test case: ${test_name}"
    log::step "  UKI mode: ${use_uki}"
    log::step "  Encryption: ${use_encryption}"
    log::step "=========================================="

    local test_workdir="${WORKDIR}/${test_name}"
    mkdir -p "${test_workdir}"

    local input_image="${test_workdir}/input.qcow2"
    local output_image="${test_workdir}/output.qcow2"
    local config_dir="${test_workdir}/config"

    # Create a copy of the input image for this test
    log::info "Creating working copy of input image..."
    if ! cp "${SOURCE_IMAGE}" "${input_image}"; then
        log::error "Failed to copy input image"
        return 1
    fi

    # Create test configuration
    create_test_config "${config_dir}"

    # Run enhancement (hardens the image before conversion)
    if ! run_enhance "${test_name}" "${input_image}"; then
        return 1
    fi

    # Run conversion
    if ! run_convert "${test_name}" "${input_image}" "${output_image}" "${config_dir}" "${use_uki}" "${use_encryption}"; then
        return 1
    fi

    # Verify the result
    if ! verify_converted_image "${test_name}" "${output_image}" "${use_uki}" "${use_encryption}"; then
        return 1
    fi

    # Clean up test-specific files to save space
    log::info "Cleaning up test files for: ${test_name}"
    rm -f "${input_image}" "${output_image}"

    log::success "Test case passed: ${test_name}"
    return 0
}

# ============================================================================
# Main
# ============================================================================

show_help() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Integration tests for cryptpilot-convert

Options:
    --case <name>   Run a specific test case. Valid cases:
                      uki-encrypted   - UKI mode with rootfs encryption
                      uki-noenc       - UKI mode without rootfs encryption
                      grub-encrypted  - GRUB mode with rootfs encryption
                      grub-noenc      - GRUB mode without rootfs encryption
    --all           Run all 4 test cases
    --input <path>  Use specified qcow2 image instead of downloading
    --help          Show this help message

Examples:
    $(basename "$0") --case uki-encrypted
    $(basename "$0") --all
    $(basename "$0") --case grub-noenc --input /path/to/image.qcow2
EOF
}

main() {
    local test_case=""
    local run_all=false
    local custom_input=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --case)
                test_case="$2"
                shift 2
                ;;
            --all)
                run_all=true
                shift
                ;;
            --input)
                custom_input="$2"
                shift 2
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            *)
                fatal "Unknown option: $1"
                ;;
        esac
    done

    # Validate custom input if provided
    if [[ -n "${custom_input}" ]]; then
        if [[ ! -f "${custom_input}" ]]; then
            fatal "Specified input image does not exist: ${custom_input}"
        fi
        log::info "Using custom input image: ${custom_input}"
    fi

    # Validate arguments
    if [[ -z "${test_case}" ]] && [[ "${run_all}" == "false" ]]; then
        show_help
        fatal "Must specify --case <name> or --all"
    fi

    if [[ -n "${test_case}" ]]; then
        case "${test_case}" in
            uki-encrypted|uki-noenc|grub-encrypted|grub-noenc)
                ;;
            *)
                fatal "Invalid test case: ${test_case}. Valid cases: uki-encrypted, uki-noenc, grub-encrypted, grub-noenc"
                ;;
        esac
    fi

    # Pre-flight checks
    log::step "Running pre-flight checks..."
    check_root
    check_tools
    check_disk_space
    if ! load_nbd_module; then
        fatal "NBD module is required but not available. Cannot proceed with tests."
    fi
    check_vg_conflict

    # Setup
    setup_workdir
    trap cleanup EXIT INT QUIT TERM

    # Set source image path
    if [[ -n "${custom_input}" ]]; then
        SOURCE_IMAGE="${custom_input}"
        log::info "Using custom input image: ${SOURCE_IMAGE}"
    else
        # Download test image if not using custom input
        download_test_image
        SOURCE_IMAGE="${TEST_IMAGE_CACHE}"
    fi

    # Run tests
    local failed_tests=()
    local passed_tests=()

    if [[ "${run_all}" == "true" ]]; then
        local all_cases=("uki-encrypted" "uki-noenc" "grub-encrypted" "grub-noenc")
        for case_name in "${all_cases[@]}"; do
            if run_test_case "${case_name}" \
                "$( [[ "${case_name}" == uki-* ]] && echo true || echo false )" \
                "$( [[ "${case_name}" == *-encrypted ]] && echo true || echo false )"; then
                passed_tests+=("${case_name}")
            else
                failed_tests+=("${case_name}")
            fi
        done
    else
        local use_uki="false"
        local use_encryption="false"
        [[ "${test_case}" == uki-* ]] && use_uki="true"
        [[ "${test_case}" == *-encrypted ]] && use_encryption="true"

        if run_test_case "${test_case}" "${use_uki}" "${use_encryption}"; then
            passed_tests+=("${test_case}")
        else
            failed_tests+=("${test_case}")
        fi
    fi

    # Report results
    echo
    log::step "=========================================="
    log::step "Test Results Summary"
    log::step "=========================================="

    if [[ ${#passed_tests[@]} -gt 0 ]]; then
        log::success "Passed tests: ${passed_tests[*]}"
    fi

    if [[ ${#failed_tests[@]} -gt 0 ]]; then
        log::error "Failed tests: ${failed_tests[*]}"
        exit 1
    fi

    log::success "All tests passed!"
    exit 0
}

main "$@"
