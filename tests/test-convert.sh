#!/bin/bash
#
# Integration tests for cryptpilot-convert
#
# This script tests the cryptpilot-convert tool's disk conversion capability.
# A single test run is defined by three independent dimensions:
#   --bootloader    uki | grub
#   --rootfs-enc    (flag) rootfs encryption enabled
#   --rootfs-noenc  (flag) rootfs encryption disabled
#   --delta-location ram | disk | disk-persist
#   --boot-matrix <list>   Space-separated "cpu:ram" tokens (e.g. "2:4G 4:8G").
#                           Reuses the converted image across the matrix. If
#                           omitted, a single boot runs with nproc and 80% of
#                           host MemTotal.
#
# Usage:
#   ./tests/test-convert.sh --rpm <path> --bootloader <uki|grub> --rootfs-enc|--rootfs-noenc --delta-location <ram|disk|disk-persist>
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

# Path to cryptpilot-fde-guest RPM package (required)
CRYPTPILOT_FDE_RPM=""

# Delta key provider for the test config: "otp" (default, recreates the delta
# every boot — preserves the original CI behavior) or "stable" (exec provider
# with the same passphrase as rootfs, so the delta persists across reboots;
# required to exercise disk-persist second-boot regressions like issue #140).
DELTA_KEY="otp"

# Whether the delta uses dm-integrity (true/false). Default false preserves the
# original behavior; true exercises the integrity AEAD path (issue #141).
INTEGRITY="false"

# When "true" and delta_location=disk-persist, run a SECOND boot of the same
# persistent overlay after the first boot reaches the login prompt. This
# catches regressions that only surface on the second boot (issue #140: the
# first boot mounts the rootfs read-write, updating s_mtime past s_lastcheck;
# the next boot's offline resize2fs then refuses with "Please run e2fsck -f").
SECOND_BOOT="false"

# When "true", skip the cryptpilot-enhance step (handy for fast local repro;
# the FDE boot-service bugs under test are independent of image hardening).
SKIP_ENHANCE="false"

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
    # virt-customize is optional: it drives cryptpilot-enhance, which skips
    # gracefully when the binary is absent (e.g. on Alinux 4, where libguestfs
    # no longer ships it). All other tools are mandatory for the convert/boot
    # flow itself.
    local tools=("wget" "qemu-img" "qemu-nbd" "cryptsetup" "lvm" "parted" "blkid" "mkfs.ext4")
    local missing=()

    for tool in "${tools[@]}"; do
        if ! command -v "$tool" &>/dev/null; then
            missing+=("$tool")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        fatal "Missing required tools: ${missing[*]}."
    fi

    if ! command -v virt-customize &>/dev/null; then
        log::info "virt-customize not found; cryptpilot-enhance will skip image hardening."
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
    if [[ -e /dev/cryptpilot ]] || vgs cryptpilot &>/dev/null; then
        fatal "LVM volume group 'cryptpilot' already exists on this host. " \
              "The test cannot run on machines with an existing 'cryptpilot' VG. " \
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
    for vg in $(vgs --noheadings -o vg_name 2>/dev/null | grep -E "^[[:space:]]*cryptpilot" || true); do
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

# Create test configuration directory. delta_key controls the delta key
# provider ("otp" recreates the delta every boot; "stable" uses an exec
# provider with the same passphrase as rootfs so the delta persists across
# reboots — required for disk-persist second-boot regression tests).
create_test_config() {
    local config_dir="$1"
    local use_encryption="$2"
    local delta_location="$3"
    local delta_key="${4:-otp}"
    local integrity="${5:-false}"
    mkdir -p "${config_dir}"

    # Delta key provider block.
    local delta_encrypt_block
    if [[ "${delta_key}" == "stable" ]]; then
        delta_encrypt_block=$(printf '[delta.encrypt.exec]\ncommand = "echo"\nargs = ["-n", "%s"]' "${TEST_PASSPHRASE}")
    else
        delta_encrypt_block='[delta.encrypt.otp]'
    fi

    if [[ "${use_encryption}" == "true" ]]; then
        cat > "${config_dir}/fde.toml" <<EOF
# Test configuration for cryptpilot-convert integration tests
[rootfs]
delta_location = "${delta_location}"

[rootfs.encrypt.exec]
command = "echo"
args = ["-n", "${TEST_PASSPHRASE}"]

[delta]
integrity = ${integrity}

${delta_encrypt_block}
EOF
    else
        cat > "${config_dir}/fde.toml" <<EOF
# Test configuration for cryptpilot-convert integration tests (no encryption)
[rootfs]
delta_location = "${delta_location}"

[delta]
integrity = ${integrity}

${delta_encrypt_block}
EOF
    fi

    log::info "Created test config at: ${config_dir}/fde.toml (delta_key=${delta_key}, integrity=${integrity})"
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

# Inject a oneshot systemd unit that reports delta-content survival to the
# serial console. On first boot it writes a random sentinel to /var/lib (which
# lives on the dm-snapshot COW, i.e. the persistent delta), syncs, and echoes
# "CRYPTPILOT_SENTINEL_WRITTEN <uuid>"; on every later boot it echoes
# "CRYPTPILOT_SENTINEL_OK <uuid>" when the sentinel survived. The sync makes
# the write reach the qcow2 overlay through dm-crypt/dm-integrity, so the
# marker survives even a forced VM kill (the power-loss case).
#
# Uses qemu-nbd + mount rather than libguestfs: Alinux 3 ships virt-customize
# in libguestfs-tools-c, but Alinux 4's merged libguestfs package does not,
# and the pre-converted rootfs partition is plain ext4 anyway.
inject_sentinel_unit() {
    local image="$1"

    local unit_file
    unit_file=$(mktemp /tmp/cryptpilot-sentinel-unit.XXXXXX)
    cat > "${unit_file}" <<'EOF'
[Unit]
Description=Verify persistent delta data survival for the cryptpilot convert test

[Service]
Type=oneshot
ExecStart=/bin/sh -c 'if [ -s /var/lib/cryptpilot-sentinel ]; then echo "CRYPTPILOT_SENTINEL_OK $(cat /var/lib/cryptpilot-sentinel)" > /dev/console; else u=$(cat /proc/sys/kernel/random/uuid); echo "$u" > /var/lib/cryptpilot-sentinel; sync; echo "CRYPTPILOT_SENTINEL_WRITTEN $u" > /dev/console; fi'

[Install]
WantedBy=multi-user.target
EOF

    local nbd_device
    nbd_device=$(get_available_nbd)
    log::info "Injecting sentinel unit into: ${image} via ${nbd_device}"
    if ! qemu-nbd --connect="${nbd_device}" "${image}"; then
        rm -f "${unit_file}"
        log::error "Failed to connect image to NBD: ${image}"
        return 1
    fi

    local mount_dir
    mount_dir=$(mktemp -d /tmp/cryptpilot-sentinel-mnt.XXXXXX)
    local inject_rc=0
    if ! ( set -e
        sleep 2
        partprobe "${nbd_device}" 2>/dev/null || true
        sleep 1
        # The pre-converted image has a plain ext4 rootfs partition. Probe
        # with blkid rather than lsblk: lsblk's FSTYPE column comes from the
        # udev database, which does not exist in the CI test container (no
        # systemd), while blkid probes the device directly.
        root_part=""
        for part in "${nbd_device}"p*; do
            [[ -b "${part}" ]] || continue
            if [[ "$(blkid -o value -s TYPE "${part}" 2>/dev/null)" == "ext4" ]]; then
                root_part="${part}"
                break
            fi
        done
        if [[ -z "${root_part}" ]]; then
            echo "no ext4 root partition found on ${nbd_device}" >&2
            exit 1
        fi
        mount "${root_part}" "${mount_dir}"
        cp "${unit_file}" "${mount_dir}/etc/systemd/system/cryptpilot-sentinel.service"
        ln -sf /etc/systemd/system/cryptpilot-sentinel.service \
            "${mount_dir}/etc/systemd/system/multi-user.target.wants/cryptpilot-sentinel.service"
        umount "${mount_dir}"
    ); then
        inject_rc=1
        umount "${mount_dir}" 2>/dev/null || true
    fi
    rmdir "${mount_dir}" 2>/dev/null || true
    rm -f "${unit_file}"
    if ! qemu-nbd --disconnect "${nbd_device}" >/dev/null 2>&1; then
        inject_rc=1
    fi

    if [[ ${inject_rc} -ne 0 ]]; then
        log::error "Failed to inject sentinel unit into: ${image}"
        return 1
    fi
    log::success "Sentinel unit injected into: ${image}"
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
        if [[ -n "${UKI_STUB_VERSION:-}" ]]; then
            cmd+=("--uki-stub-version" "${UKI_STUB_VERSION}")
        fi
    fi

    if [[ "${use_encryption}" == "true" ]]; then
        cmd+=("--rootfs-passphrase" "${TEST_PASSPHRASE}")
    else
        cmd+=("--rootfs-no-encryption")
    fi

    cmd+=("--package" "${CRYPTPILOT_FDE_RPM}")

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

    local verify_failed=0

    # Test reference value calculation
    log::info "Testing reference value calculation..."
    if command -v cryptpilot-fde-host &>/dev/null; then
        local reference_value_file="${WORKDIR}/reference_value-${test_name}.json"
        local reference_value_stderr="${WORKDIR}/reference_value-${test_name}.stderr"
        if cryptpilot-fde-host show-reference-value --disk "${output_image}" 1>"${reference_value_file}" 2>"${reference_value_stderr}"; then
            log::info "Reference value calculation succeeded"
            cat "${reference_value_file}"
        else
            log::error "Reference value calculation failed"
            log::error "stderr: $(cat "${reference_value_stderr}" 2>/dev/null)"
            verify_failed=1
        fi
    else
        log::warn "cryptpilot-fde-host not found, skipping reference value test"
    fi

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

    # Check partition layout
    log::info "Checking partition layout..."
    if ! lsblk "${nbd_device}"; then
        log::error "Failed to list partitions"
        verify_failed=1
    fi

    # Check for LVM partition and volume group
    log::info "Scanning for LVM..."
    pvscan --cache 2>/dev/null || true
    vgscan 2>/dev/null || true

    if ! vgs cryptpilot &>/dev/null; then
        log::error "LVM volume group 'cryptpilot' not found"
        verify_failed=1
    else
        log::info "LVM volume group 'cryptpilot' found"

        # Check for logical volumes
        if ! lvs cryptpilot/rootfs &>/dev/null; then
            log::error "Logical volume 'cryptpilot/rootfs' not found"
            verify_failed=1
        else
            log::info "Logical volume 'cryptpilot/rootfs' found"
        fi

        if ! lvs cryptpilot/rootfs_hash &>/dev/null; then
            log::error "Logical volume 'cryptpilot/rootfs_hash' not found"
            verify_failed=1
        else
            log::info "Logical volume 'cryptpilot/rootfs_hash' found"
        fi
    fi

    # Check encryption status
    if [[ "${use_encryption}" == "true" ]]; then
        log::info "Checking LUKS encryption..."
        vgchange -ay cryptpilot 2>/dev/null || true
        if cryptsetup isLuks /dev/mapper/cryptpilot-rootfs 2>/dev/null; then
            log::info "LUKS encryption verified on cryptpilot-rootfs"
        else
            log::error "Expected LUKS encryption on cryptpilot-rootfs but not found"
            verify_failed=1
        fi
    else
        log::info "Skipping encryption check (no-encryption mode)"
    fi

    # Cleanup: deactivate VG and disconnect NBD
    log::info "Cleaning up verification mounts..."
    vgchange -an cryptpilot 2>/dev/null || true
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


# Ensure a usable container runtime for the QEMU boot test.
#
# On Alinux 3, the "docker" package is actually podman-docker: a daemonless
# podman emulation, so `docker` works out of the box with no running daemon.
# On Alinux 4, "docker" is the real Docker Engine, which needs a running
# dockerd. But the CI test container is started with `sleep infinity` (no
# systemd), so the daemon is never started and `docker run` fails with
# "Cannot connect to the Docker daemon at unix:///var/run/docker.sock".
#
# When `docker info` already works (podman emulation or a running dockerd),
# this is a no-op. Otherwise it launches dockerd in the background. The flags
# keep it functional inside a privileged nested container: vfs avoids
# overlay-in-nested-container issues; bridge/iptables/masquerading are
# disabled because dockerd's default bridge setup fails in the nested
# container. With no bridge, containers get no `eth0`, so the QEMU boot
# container is launched with `--network=host` (see test_qemu_boot) so the
# qemus entrypoint can find a network interface; qemu guest networking uses
# SLIRP in-process, independent of the container network. Sets
# DOCKERD_STARTED=1 so the caller knows to apply --network=host.
ensure_docker_runtime() {
    if docker info >/dev/null 2>&1; then
        return 0
    fi

    if ! command -v dockerd >/dev/null 2>&1; then
        log::error "docker is installed but dockerd is missing and no podman emulation is available; cannot run the QEMU boot test."
        return 1
    fi

    log::info "dockerd is not running (no systemd in the test container); starting dockerd in background..."
    dockerd \
        --host=unix:///var/run/docker.sock \
        --storage-driver=vfs \
        --iptables=false \
        --bridge=none \
        --ip-masq=false \
        >/tmp/.cryptpilot-dockerd.log 2>&1 &
    local dockerd_pid=$!

    local waited=0
    while ! docker info >/dev/null 2>&1; do
        waited=$((waited + 1))
        if [[ $waited -gt 30 ]]; then
            log::error "dockerd did not become ready within 30s (see /tmp/.cryptpilot-dockerd.log)."
            return 1
        fi
        if ! kill -0 "$dockerd_pid" 2>/dev/null; then
            log::error "dockerd exited unexpectedly (see /tmp/.cryptpilot-dockerd.log)."
            return 1
        fi
        sleep 1
    done
    log::success "dockerd is ready (pid ${dockerd_pid})."
    DOCKERD_STARTED=1
    return 0
}

# Test booting the converted image with QEMU in container
# Returns 0 if login prompt appears, 1 if emergency shell or timeout
test_qemu_boot() {
    local test_name="$1"
    local output_image="$2"
    local cpu_cores="${3:-$(nproc)}"
    # Pin guest RAM via the 4th arg (matrix loop) or fall back to 80% of host
    # MemTotal (read from /proc/meminfo of this privileged runtime container,
    # which reflects the CI runner's physical memory). A low value (e.g. 4G)
    # surfaces the UKI "SizeOfImage hole" class of regressions, since UEFI
    # LoadImage() must allocate SizeOfImage bytes of contiguous memory up front.
    local ram_size="${4:-$(awk '/MemTotal/{printf "%d", $2 * 0.8 / 1024}' /proc/meminfo)}"

    log::step "Testing QEMU boot for: ${test_name} (cpu=${cpu_cores}, ram=${ram_size})"

    # Alinux 4 ships real Docker (not podman-docker) and the test container
    # has no init system, so dockerd must be started explicitly. No-op on
    # Alinux 3 where docker is the daemonless podman emulation.
    if ! ensure_docker_runtime; then
        return 1
    fi

    local boot_log="${WORKDIR}/${test_name}-cpu${cpu_cores}-ram${ram_size}-boot.log"
    log::info "Starting QEMU container with UEFI boot mode (CPU_CORES=${cpu_cores}, RAM_SIZE=${ram_size})"

    # Start QEMU container in background
    local container_name="qemu-test-${test_name}-cpu${cpu_cores}-ram${ram_size}-$$"
    # When we launched dockerd ourselves (Alinux 4), it runs with no bridge, so
    # containers get no eth0 and the qemus entrypoint aborts asking for
    # VM_NET_DEV. Use the host network namespace so qemus can find an
    # interface. Not needed (and harmless) under podman on Alinux 3, but only
    # applied when DOCKERD_STARTED is set to avoid changing the working path.
    local docker_net_args=()
    if [[ "${DOCKERD_STARTED:-0}" == "1" ]]; then
        docker_net_args+=(--network=host)
    fi
    if ! docker run -d --rm --privileged \
        "${docker_net_args[@]}" \
        -v "${output_image}:${output_image}:ro" \
        -e "IMAGE=${output_image}" \
        -e BOOT="" \
        -e "KVM=N" \
        -e "CPU_CORES=${cpu_cores}" \
        -e "RAM_SIZE=${ram_size}" \
        --entrypoint /bin/bash \
        --name "${container_name}" \
        ghcr.io/qemus/qemu:7.29 \
            -c 'echo "📦 Creating temporary COW layer..." && \
            qemu-img create -f qcow2 -F qcow2 -b ${IMAGE} /boot.qcow2 && \
            echo "✅ COW layer created, starting QEMU..." && \
            exec /usr/bin/tini -s /run/entry.sh'; then
        log::error "Failed to start QEMU container: ${container_name}"
        return 1
    fi

    log::info "QEMU container started: ${container_name}"

    # Stream logs to file and check for boot status
    local timeout=540  # 9 minutes; TCG under GitHub runner contention can push a 2-min boot well past 6.
    local elapsed=0
    local check_interval=2
    local boot_success=false

    # Start capturing logs in background
    docker logs -f "${container_name}" > "${boot_log}" 2>&1 &
    local logs_pid=$!

    while [[ $elapsed -lt $timeout ]]; do
        sleep $check_interval
        elapsed=$((elapsed + check_interval))

        # Check if container is still running
        if ! docker ps -q --filter "name=${container_name}" | grep -q .; then
            log::warn "QEMU container exited prematurely"
            break
        fi

        # Check for login prompt (success)
        if grep -q -i " on an x86_64" "${boot_log}" 2>/dev/null; then
            log::success "Login prompt detected - boot successful!"
            boot_success=true
            break
        fi

        # Check for emergency shell (failure)
        if grep -q -i "Emergency Mode" "${boot_log}" 2>/dev/null || \
           grep -q -i "emergency shell" "${boot_log}" 2>/dev/null || \
           grep -q -i "Entering emergency mode" "${boot_log}" 2>/dev/null; then
            log::error "Emergency shell detected - boot failed!"
            break
        fi

        # Check for kernel panic
        if grep -q -i "Kernel panic" "${boot_log}" 2>/dev/null; then
            log::error "Kernel panic detected - boot failed!"
            break
        fi

        # Check for UEFI StartImage failure: OVMF loaded the UKI but
        # StartImage returned "Load Error" (e.g. stub/kernel handover
        # incompatibility), then dropped to the EFI shell. This fails in
        # seconds; without this marker the loop would wait the full timeout.
        if grep -q -i "failed to start Boot.*: Load Error" "${boot_log}" 2>/dev/null || \
           grep -q -i "UEFI Interactive Shell" "${boot_log}" 2>/dev/null; then
            log::error "UEFI StartImage failed (Load Error / EFI shell) - boot failed!"
            break
        fi
    done

    # Stop log capture (kill the docker logs process)
    kill $logs_pid 2>/dev/null || true
    wait $logs_pid 2>/dev/null || true

    # Stop and remove container
    log::info "Stopping QEMU container..."
    docker stop "${container_name}" >/dev/null 2>&1 || true

    # Show full boot log for debugging
    log::info "Full boot log:"
    cat "${boot_log}" || true

    if [[ "${boot_success}" == "true" ]]; then
        log::success "QEMU boot test passed for: ${test_name}"
        return 0
    else
        if [[ $elapsed -ge $timeout ]]; then
            log::error "QEMU boot test timed out after ${timeout} seconds"
        fi
        log::error "QEMU boot test failed for: ${test_name}"
        return 1
    fi
}

# Boot an already-converted image once with a direct qemu invocation (TCG,
# UEFI) inside the qemus container. Bypasses the qemus entrypoint so the SAME
# image (and its persistent disk-persist delta) can be booted twice — needed
# for issue #140, whose second-boot failure requires the first boot's writes
# to persist in the same qcow2 overlay.
#
# Uses if=virtio + OVMF_CODE_4M.fd / a writable copy of OVMF_VARS_4M.fd.
# Returns 0 if the login prompt appears, 1 on emergency/panic/timeout.
# An optional 5th argument is a fixed-string pattern that must ALSO appear
# in the boot log before the boot counts as successful (used by the persist
# test to require the sentinel marker, which lands on the console moments
# after the login prompt).
# Writes the serial log to ${WORKDIR}/${test_name}-direct-boot.log.
test_qemu_boot_direct() {
    local test_name="$1"
    local image="$2"
    local cpu_cores="${3:-4}"
    local ram_size="${4:-4G}"
    local require_pattern="${5:-}"

    local boot_log="${WORKDIR}/${test_name}-direct-boot.log"
    local image_bn; image_bn=$(basename "${image}")
    local container_name="qemu-direct-${test_name}-$$"

    log::step "Direct QEMU boot for: ${test_name} (cpu=${cpu_cores}, ram=${ram_size}, image=${image_bn})"

    # Alinux 4 ships real Docker (not podman-docker) and the test container
    # has no init system, so dockerd must be started explicitly. No-op on
    # Alinux 3 where docker is the daemonless podman emulation. A self-started
    # dockerd has no bridge, so the QEMU container needs the host network
    # namespace for guest user-mode networking (NTP time sync).
    if ! ensure_docker_runtime; then
        return 1
    fi
    local docker_net_args=()
    if [[ "${DOCKERD_STARTED:-0}" == "1" ]]; then
        docker_net_args+=(--network=host)
    fi

    docker rm -f "${container_name}" 2>/dev/null || true
    docker run -d --rm --privileged \
        "${docker_net_args[@]}" \
        -v "$(dirname "${image}"):/diskdir:rw" \
        --name "${container_name}" --entrypoint /bin/bash \
        "ghcr.io/qemus/qemu:7.29" -c '
set -e
cp /usr/share/OVMF/OVMF_VARS_4M.fd /tmp/vars.fd
exec qemu-system-x86_64 \
  -accel tcg -cpu max -smp '"${cpu_cores}"' -m '"${ram_size}"' \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,file=/tmp/vars.fd \
  -drive file=/diskdir/'"${image_bn}"',format=qcow2,if=virtio \
  -nographic -serial mon:stdio -no-reboot
' > /dev/null 2>&1

    docker logs -f "${container_name}" > "${boot_log}" 2>&1 &
    local log_pid=$!

    local timeout=540 elapsed=0 boot_success=false
    while [[ $elapsed -lt $timeout ]]; do
        sleep 5; elapsed=$((elapsed+5))
        if grep -q -i " on an x86_64" "${boot_log}" 2>/dev/null; then
            if [[ -z "${require_pattern}" ]] || grep -q -F "${require_pattern}" "${boot_log}" 2>/dev/null; then
                log::success "Login prompt detected - boot successful!"
                boot_success=true
                break
            fi
            # Login is up but the required marker has not appeared yet; keep
            # waiting until it does or the timeout expires.
        fi
        # NOTE: bare "INTEGRITY AEAD ERROR" is not a boot-failure marker:
        # blkid probing the end of a freshly created delta reads sectors
        # whose integrity tags were never initialized, and those errors are
        # noise. The fatal markers are the dmsetup failure, emergency mode,
        # or the FDE service failing.
        if grep -qi "Emergency Mode\|emergency shell\|Kernel panic\|Failed to setup volumes required by FDE\|Failed to create dm-snapshot device\|reload ioctl .* failed\|Please run 'e2fsck\|Failed to resize ext4\|Failed to start Cryptpilot FDE" "${boot_log}" 2>/dev/null; then
            log::error "Boot failure detected - boot failed!"
            break
        fi
        docker ps -q --filter "name=${container_name}" | grep -q . || { log::error "QEMU container exited unexpectedly"; break; }
    done

    kill "${log_pid}" 2>/dev/null || true
    wait "${log_pid}" 2>/dev/null || true
    docker rm -f "${container_name}" >/dev/null 2>&1 || true

    log::info "Full direct boot log:"
    cat "${boot_log}" || true

    if [[ "${boot_success}" == "true" ]]; then
        log::success "Direct QEMU boot test passed for: ${test_name}"
        return 0
    fi
    if [[ -n "${require_pattern}" ]] && grep -q -i " on an x86_64" "${boot_log}" 2>/dev/null; then
        log::error "Login prompt appeared but the required marker never did: ${require_pattern}"
    fi
    log::error "Direct QEMU boot test failed for: ${test_name}"
    return 1
}

# Issue #140 regression: a disk-persist + stable-key + integrity=false image
# must boot, then RE-boot the same persistent overlay and still boot. On
# unfixed code the first boot updates s_mtime past s_lastcheck; the second
# boot's offline resize2fs rejects it ("Please run 'e2fsck'") -> emergency.
# A persistent qcow2 overlay (relative backing filename, so qemu resolves it
# inside the container) carries the first boot's writes into the second boot.
#
# The boots also verify delta CONTENT survival via the sentinel unit
# (inject_sentinel_unit): the first boot's sentinel must still be present
# with the same UUID after a forced VM kill (the power-loss case), and a
# third boot after simulate_interrupted_delta_init() must recover to a
# working snapshot from the half-initialized state (issue #141's
# interrupted-initialization case: LUKS volume initialized and marked,
# snapshot header never committed).
test_qemu_boot_persist_second() {
    local test_name="$1"
    local output_image="$2"
    local cpu_cores="${3:-4}"
    local ram_size="${4:-4G}"

    # The overlay must live in the SAME directory as the output image: the
    # backing file is referenced by its bare name so qemu resolves it inside
    # the boot container (host workdir <-> container /diskdir), and the boot
    # container only mounts that one directory.
    local image_dir; image_dir="$(dirname "${output_image}")"
    local overlay="${image_dir}/${test_name}-persist-overlay.qcow2"
    rm -f "${overlay}"
    if ! ( cd "${image_dir}" \
        && qemu-img create -f qcow2 -F qcow2 -b "$(basename "${output_image}")" "$(basename "${overlay}")" >/dev/null ); then
        log::error "Failed to create persistent overlay in ${image_dir} for: ${test_name}"
        return 1
    fi

    log::step "Persist boot test for: ${test_name}"
    log::info "Boot #1 (expect login + sentinel written through the persistent delta)"
    if ! test_qemu_boot_direct "${test_name}-b1" "${overlay}" "${cpu_cores}" "${ram_size}" "CRYPTPILOT_SENTINEL_WRITTEN"; then
        log::error "Persist boot #1 failed for: ${test_name}"
        return 1
    fi
    local b1_log="${WORKDIR}/${test_name}-b1-direct-boot.log"
    local sentinel_uuid
    sentinel_uuid=$(grep -aoE 'CRYPTPILOT_SENTINEL_WRITTEN [0-9a-f-]{36}' "${b1_log}" 2>/dev/null | head -1 | awk '{print $2}')
    if [[ -z "${sentinel_uuid}" ]]; then
        log::error "Boot #1 reached login but wrote no sentinel; was the sentinel unit injected?"
        return 1
    fi
    log::info "Sentinel written: ${sentinel_uuid}"

    log::info "Boot #2 (same overlay after a forced kill; expect login + identical sentinel = data survived)"
    if ! test_qemu_boot_direct "${test_name}-b2" "${overlay}" "${cpu_cores}" "${ram_size}" "CRYPTPILOT_SENTINEL_OK ${sentinel_uuid}"; then
        log::error "Persist boot #2 failed for: ${test_name} (boot failure, or sentinel lost across the forced kill)"
        return 1
    fi

    log::info "Simulating an interrupted delta initialization (blank COW header, LUKS still initialized)"
    if ! simulate_interrupted_delta_init "${overlay}"; then
        log::error "Failed to simulate the interrupted delta initialization for: ${test_name}"
        return 1
    fi
    # The blank COW header means the recovery path reinitializes the delta, so
    # the previous sentinel is gone by design; this boot only requires login.
    log::info "Boot #3 (recovery; expect login from the reinitialized snapshot)"
    if ! test_qemu_boot_direct "${test_name}-b3" "${overlay}" "${cpu_cores}" "${ram_size}"; then
        log::error "Persist boot #3 failed for: ${test_name} (interrupted-initialization recovery is broken)"
        return 1
    fi

    log::success "Persist boot test passed for: ${test_name}"
    return 0
}

# Simulate a boot that crashed between marking the delta LUKS volume as
# initialized and committing the dm-snapshot COW header: the COW header chunk
# is blanked (16 sectors = 8 KiB, one snapshot chunk) while the LUKS volume
# keeps its initialized marker. The next boot must recognize the readable
# blank header and reinitialize the snapshot instead of failing or wiping a
# volume it cannot interpret.
#
# Runs from the test container: connects the overlay via qemu-nbd, opens the
# delta LUKS volume read-write (dm-integrity journal replay is a write, a
# read-only open cannot apply it), zeroes the header chunk through the
# mapping, and closes everything again.
simulate_interrupted_delta_init() {
    local overlay="$1"

    modprobe dm-integrity 2>/dev/null || true
    if ! command -v cryptsetup >/dev/null 2>&1; then
        log::error "cryptsetup is required to simulate the interrupted delta initialization"
        return 1
    fi

    local nbd_device
    nbd_device=$(get_available_nbd)
    log::info "Simulating interrupted delta init on ${overlay} via ${nbd_device}"

    if ! qemu-nbd --connect="${nbd_device}" "${overlay}"; then
        log::error "Failed to connect overlay to NBD"
        return 1
    fi

    local cleanup_rc=0
    if ! ( set -e
        sleep 2
        partprobe "${nbd_device}" 2>/dev/null || true
        sleep 1
        vgchange -ay cryptpilot >/dev/null 2>&1
        echo -n "${TEST_PASSPHRASE}" | cryptsetup open /dev/cryptpilot/delta cryptpilot_int
        # One dm-snapshot chunk: 16 sectors of 512 bytes (the chunk size used
        # by the snapshot table in the guest boot service).
        dd if=/dev/zero of=/dev/mapper/cryptpilot_int bs=512 count=16 conv=fsync status=none
        cryptsetup close cryptpilot_int
        vgchange -an cryptpilot >/dev/null 2>&1
    ); then
        cleanup_rc=1
        # Best-effort cleanup of whatever step failed midway.
        cryptsetup close cryptpilot_int >/dev/null 2>&1 || true
        vgchange -an cryptpilot >/dev/null 2>&1 || true
    fi

    if ! qemu-nbd --disconnect "${nbd_device}" >/dev/null 2>&1; then
        cleanup_rc=1
    fi

    if [[ ${cleanup_rc} -ne 0 ]]; then
        log::error "Failed to blank the COW header chunk on the delta volume"
        return 1
    fi
    log::success "COW header chunk blanked (interrupted initialization simulated)"
    return 0
}

# Boot the already-converted image across a vCPU/RAM matrix, reusing the
# single output.qcow2 (each boot layers a fresh COW on the read-only base),
# so the expensive convert runs once and only the cheap boot is repeated.
# Matrix entries are "cpu:ram" tokens (ram is a QEMU size, e.g. 4G or 16384M).
# Set via the --boot-matrix option. When empty, a single boot runs with the
# host's nproc and 80% of MemTotal (the original auto behavior).
test_qemu_boot_matrix() {
    local test_name="$1"
    local output_image="$2"
    local matrix="${BOOT_MATRIX:-}"

    # No matrix given: single boot, auto cpu/ram (original behavior).
    if [[ -z "$matrix" ]]; then
        test_qemu_boot "$test_name" "$output_image"
        return $?
    fi

    local combo cpu ram
    for combo in $matrix; do
        cpu="${combo%%:*}"
        ram="${combo##*:}"
        if [[ -z "$cpu" || -z "$ram" || "$combo" != *:* ]]; then
            log::error "Invalid boot matrix entry '${combo}' (expected cpu:ram, e.g. 2:4G)"
            return 1
        fi
        # NOTE: 16G guest RAM needs a runner with >=16G physical RAM; on a
        # 16G runner it sits at the OOM edge and may fail flakily.
        log::step "Boot matrix entry: cpu=${cpu}, ram=${ram}"
        # Retry once per combo: TCG on contended GitHub runners makes the
        # occasional boot exceed the timeout even though the image is fine,
        # and a retry usually clears it.
        local attempt
        for attempt in 1 2; do
            if test_qemu_boot "$test_name" "$output_image" "$cpu" "$ram"; then
                break
            fi
            if [[ "$attempt" -eq 1 ]]; then
                log::warn "Boot combo cpu=${cpu}, ram=${ram} failed (attempt 1/2), retrying..."
                continue
            fi
            log::error "Boot matrix failed at cpu=${cpu}, ram=${ram} for: ${test_name}"
            return 1
        done
    done
    return 0
}


# ============================================================================
# Test case functions
# ============================================================================

run_test_case() {
    local test_name="$1"
    local use_uki="$2"
    local use_encryption="$3"
    local delta_location="$4"
    local delta_key="${5:-${DELTA_KEY}}"
    local integrity="${6:-${INTEGRITY}}"
    local second_boot="${7:-${SECOND_BOOT}}"
    local skip_enhance="${8:-${SKIP_ENHANCE}}"

    log::step "=========================================="
    log::step "Running test case: ${test_name}"
    log::step "  UKI mode: ${use_uki}"
    log::step "  Encryption: ${use_encryption}"
    log::step "  Delta location: ${delta_location}"
    log::step "=========================================="

    local test_workdir="${WORKDIR}/${test_name}"
    mkdir -p "${test_workdir}"

    local input_image="${test_workdir}/input.qcow2"
    local output_image="${test_workdir}/output.qcow2"
    local config_dir="${test_workdir}/config"

    # Create a working copy of the input image for this test
    # Use qemu-img with backing file for fast copy-on-write clone
    log::info "Creating working copy of input image (using qcow2 backing file)..."
    if ! qemu-img create -f qcow2 -F qcow2 -b "${SOURCE_IMAGE}" "${input_image}"; then
        log::error "Failed to create input image with qemu-img"
        return 1
    fi

    # Create test configuration
    create_test_config "${config_dir}" "${use_encryption}" "${delta_location}" "${delta_key}" "${integrity}"

    # Run enhancement (hardens the image before conversion). Skippable for
    # fast local repro since the FDE boot-service bugs under test do not
    # depend on image hardening.
    if [[ "${skip_enhance}" != "true" ]]; then
        if ! run_enhance "${test_name}" "${input_image}"; then
            return 1
        fi
    else
        log::info "Skipping cryptpilot-enhance (--skip-enhance)"
    fi

    # The second-boot persistence test asserts delta content survival via a
    # sentinel, so the guest needs the reporting unit before conversion.
    if [[ "${second_boot}" == "true" ]]; then
        if ! inject_sentinel_unit "${input_image}"; then
            return 1
        fi
    fi

    # Run conversion
    if ! run_convert "${test_name}" "${input_image}" "${output_image}" "${config_dir}" "${use_uki}" "${use_encryption}"; then
        return 1
    fi

    # Free input image and source immediately after conversion to reclaim disk space.
    # output.qcow2 is a standalone image that no longer depends on these files.
    log::info "Freeing input image and source to reclaim disk space..."
    rm -f "${input_image}"
    rm -f "${SOURCE_IMAGE}"

    # Verify the result
    if ! verify_converted_image "${test_name}" "${output_image}" "${use_uki}" "${use_encryption}"; then
        return 1
    fi

    # Test QEMU boot. For the disk-persist + second-boot regression (issue
    # #140), boot the same persistent overlay twice instead of the single-boot
    # matrix; otherwise use the vCPU/RAM matrix (each boot layers a fresh COW
    # on the read-only base, so the expensive convert runs once).
    if [[ "${second_boot}" == "true" && "${delta_location}" == "disk-persist" ]]; then
        if ! test_qemu_boot_persist_second "${test_name}" "${output_image}"; then
            return 1
        fi
    else
        if ! test_qemu_boot_matrix "${test_name}" "${output_image}"; then
            return 1
        fi
    fi

    # Clean up remaining test-specific files
    log::info "Cleaning up test files for: ${test_name}"
    rm -f "${output_image}"

    log::success "Test case passed: ${test_name}"
    return 0
}

# ============================================================================
# Main
# ============================================================================

show_help() {
    cat <<EOF
Usage: $(basename "$0") --rpm <path> --bootloader <uki|grub> --rootfs-enc|--rootfs-noenc --delta-location <ram|disk|disk-persist> [OPTIONS]

Integration tests for cryptpilot-convert

Required:
    --rpm <path>              Path to cryptpilot-fde-guest RPM package
    --bootloader <uki|grub>   Boot mode
    --rootfs-enc              Enable rootfs encryption
    --rootfs-noenc            Disable rootfs encryption
    --delta-location <value>  Delta partition location: ram | disk | disk-persist

Options:
    --input <path>   Use specified qcow2 image instead of downloading
    --boot-matrix <list>  Space-separated "cpu:ram" tokens to boot the converted
                     image with, e.g. "2:4G 4:8G 8:16G 12:16G 16:16G". The
                     converted image is reused across the matrix. When omitted,
                     a single boot runs with the host's nproc and 80% of MemTotal.
    --uki-stub-version <ver>  Pin the systemd UEFI stub version (e.g. 258) used
                     to assemble the UKI. Only meaningful with --bootloader uki.
                     When omitted, the distro stub is used.
    --delta-key <otp|stable>  Delta key provider. "otp" (default) recreates the
                     delta every boot; "stable" uses an exec provider with the
                     same passphrase as rootfs so the delta persists across
                     reboots (required for real disk-persist / --second-boot).
    --integrity <true|false>  Enable dm-integrity on the delta (default false).
                     true exercises the integrity AEAD path (issue #141).
    --second-boot  With --delta-location disk-persist: after the first boot
                     reaches login, boot the SAME persistent overlay again.
                     Catches second-boot regressions like issue #140.
    --skip-enhance  Skip the cryptpilot-enhance step (fast local repro).
    --help           Show this help message

Examples:
    $(basename "$0") --rpm ./cryptpilot-fde-guest-*.rpm --bootloader uki --rootfs-enc --delta-location ram
    $(basename "$0") --rpm ./cryptpilot-fde-guest-*.rpm --bootloader grub --rootfs-noenc --delta-location disk --input /path/to/image.qcow2
    # Issue #141 regression (disk-persist + integrity=true, first-boot failure):
    $(basename "$0") --rpm ./cryptpilot-fde-guest-*.rpm --bootloader uki --rootfs-enc --delta-location disk-persist --delta-key stable --integrity true
    # Issue #140 regression (disk-persist, stable key, second-boot resize failure):
    $(basename "$0") --rpm ./cryptpilot-fde-guest-*.rpm --bootloader uki --rootfs-enc --delta-location disk-persist --delta-key stable --second-boot
EOF
}

main() {
    local bootloader=""
    local rootfs_enc=""
    local delta_location=""
    local custom_input=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --rpm)
                CRYPTPILOT_FDE_RPM="$2"
                shift 2
                ;;
            --bootloader)
                bootloader="$2"
                shift 2
                ;;
            --rootfs-enc)
                rootfs_enc="enc"
                shift
                ;;
            --rootfs-noenc)
                rootfs_enc="noenc"
                shift
                ;;
            --delta-location)
                delta_location="$2"
                shift 2
                ;;
            --input)
                custom_input="$2"
                shift 2
                ;;
            --boot-matrix)
                BOOT_MATRIX="$2"
                shift 2
                ;;
            --uki-stub-version)
                UKI_STUB_VERSION="$2"
                shift 2
                ;;
            --delta-key)
                DELTA_KEY="$2"
                shift 2
                ;;
            --integrity)
                INTEGRITY="$2"
                shift 2
                ;;
            --second-boot)
                SECOND_BOOT="true"
                shift
                ;;
            --skip-enhance)
                SKIP_ENHANCE="true"
                shift
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

    # Validate required --rpm argument
    if [[ -z "${CRYPTPILOT_FDE_RPM}" ]]; then
        show_help
        fatal "Missing required argument: --rpm <path>"
    fi
    if [[ ! -f "${CRYPTPILOT_FDE_RPM}" ]]; then
        fatal "cryptpilot-fde-guest RPM package not found: ${CRYPTPILOT_FDE_RPM}"
    fi
    log::info "Using cryptpilot-fde-guest RPM: ${CRYPTPILOT_FDE_RPM}"

    # Validate --bootloader
    if [[ "${bootloader}" != "uki" && "${bootloader}" != "grub" ]]; then
        show_help
        fatal "Invalid or missing --bootloader: must be 'uki' or 'grub'"
    fi

    # --uki-stub-version only matters for UKI builds.
    if [[ -n "${UKI_STUB_VERSION:-}" && "${bootloader}" != "uki" ]]; then
        fatal "--uki-stub-version is only meaningful with --bootloader uki"
    fi

    # Validate --rootfs-enc / --rootfs-noenc
    if [[ -z "${rootfs_enc}" ]]; then
        show_help
        fatal "Must specify --rootfs-enc or --rootfs-noenc"
    fi

    # Validate --delta-location
    if [[ "${delta_location}" != "ram" && "${delta_location}" != "disk" && "${delta_location}" != "disk-persist" ]]; then
        show_help
        fatal "Invalid or missing --delta-location: must be 'ram', 'disk', or 'disk-persist'"
    fi

    # Validate --delta-key
    if [[ "${DELTA_KEY}" != "otp" && "${DELTA_KEY}" != "stable" ]]; then
        fatal "Invalid --delta-key: must be 'otp' or 'stable'"
    fi

    # Validate --integrity
    if [[ "${INTEGRITY}" != "true" && "${INTEGRITY}" != "false" ]]; then
        fatal "Invalid --integrity: must be 'true' or 'false'"
    fi

    # --second-boot only makes sense for disk-persist (the delta must persist
    # across boots; ram/disk recreate it every boot).
    if [[ "${SECOND_BOOT}" == "true" && "${delta_location}" != "disk-persist" ]]; then
        fatal "--second-boot requires --delta-location disk-persist"
    fi

    # stable key is strongly recommended with disk-persist, otherwise the delta
    # is recreated every boot and persistence/second-boot semantics are lost.
    if [[ "${delta_location}" == "disk-persist" && "${DELTA_KEY}" == "otp" ]]; then
        log::warn "disk-persist with otp key recreates the delta every boot (no persistence); use --delta-key stable for real persistence"
    fi

    # Validate custom input if provided
    if [[ -n "${custom_input}" ]]; then
        if [[ ! -f "${custom_input}" ]]; then
            fatal "Specified input image does not exist: ${custom_input}"
        fi
        log::info "Using custom input image: ${custom_input}"
    fi

    # Derive test parameters
    local use_uki="false"
    local use_encryption="false"
    [[ "${bootloader}" == "uki" ]] && use_uki="true"
    [[ "${rootfs_enc}" == "enc" ]] && use_encryption="true"
    local test_name="${bootloader}-${rootfs_enc}-${delta_location}"
    # Append a suffix for non-default options so logs/artifacts are distinct.
    [[ "${DELTA_KEY}" == "stable" ]] && test_name="${test_name}-stable"
    [[ "${INTEGRITY}" == "true" ]] && test_name="${test_name}-integ"
    [[ "${SECOND_BOOT}" == "true" ]] && test_name="${test_name}-2boot"

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

    # Run test
    local failed_tests=()
    local passed_tests=()

    if run_test_case "${test_name}" "${use_uki}" "${use_encryption}" "${delta_location}" "${DELTA_KEY}" "${INTEGRITY}" "${SECOND_BOOT}" "${SKIP_ENHANCE}"; then
        passed_tests+=("${test_name}")
    else
        failed_tests+=("${test_name}")
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
