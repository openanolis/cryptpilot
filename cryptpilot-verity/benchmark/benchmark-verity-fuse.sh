#!/bin/bash
#
# cryptpilot-verity FUSE performance benchmark script
# Compare: baseline vs verity-fuse vs cachefs vs cachefs+verity-fuse
#
# Each test runs RUN_COUNT times with full remount between runs to avoid cache effects
#

set -e

# ==================== Configuration ====================
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCHMARK_DIR="$SCRIPT_DIR"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Prefer project-built cryptpilot-verity
if [ -x "$PROJECT_ROOT/target/release/cryptpilot-verity" ]; then
    CRYPTPILOT_VERITY="$PROJECT_ROOT/target/release/cryptpilot-verity"
elif [ -x "$PROJECT_ROOT/target/debug/cryptpilot-verity" ]; then
    CRYPTPILOT_VERITY="$PROJECT_ROOT/target/debug/cryptpilot-verity"
else
    CRYPTPILOT_VERITY="cryptpilot-verity"
fi

# Test directories
TEST_DATA_DIR="${TEST_DATA_DIR:-$BENCHMARK_DIR/data}"
TEST_MOUNT_POINT="${TEST_MOUNT_POINT:-$BENCHMARK_DIR/mount}"
RESULT_DIR="${RESULT_DIR:-$BENCHMARK_DIR/results}"
LOG_DIR="${LOG_DIR:-$BENCHMARK_DIR/logs}"

# Encrypted data directories
ENCRYPTED_DATA_DIR="$BENCHMARK_DIR/encrypted_data"
VERITY_ENCRYPTED_DATA_DIR="$BENCHMARK_DIR/verity_encrypted_data"
CACHEFS_MOUNT_POINT="$BENCHMARK_DIR/cachefs_mount"
CACHEFS_VERITY_MOUNT_POINT="$BENCHMARK_DIR/cachefs_verity_mount"
VERITY_ON_CACHEFS_MOUNT_POINT="$BENCHMARK_DIR/verity_on_cachefs_mount"

# Test parameters
RUN_COUNT=3
SMALL_FILE_COUNT=1000
MEDIUM_FILE_COUNT=100
LARGE_FILE_COUNT=5

# Encryption password (for gocryptfs)
GOCRYPTFS_PASSWORD="benchmark_test_password_12345"

# cachefs container image
CACHEFS_IMAGE="eci-nydus-registry.cn-hangzhou.cr.aliyuncs.com/kangaroo/cachefs:1.0.7-2.0.4d67d16"

# ==================== Color output ====================
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# ==================== Helper functions ====================

drop_caches() {
    sync
    echo 3 > /proc/sys/vm/drop_caches 2>/dev/null || {
        log_warn "Cannot drop caches, need root privileges"
        return 1
    }
}

calc_average() {
    local arr=("$@")
    local sum=0
    local count=${#arr[@]}
    for val in "${arr[@]}"; do
        sum=$(echo "$sum + $val" | bc -l)
    done
    echo "scale=3; $sum / $count" | bc -l
}

calc_stddev() {
    local arr=("$@")
    local avg=$(calc_average "${arr[@]}")
    local count=${#arr[@]}
    local sum_sq=0
    for val in "${arr[@]}"; do
        local diff=$(echo "$val - $avg" | bc -l)
        sum_sq=$(echo "$sum_sq + ($diff * $diff)" | bc -l)
    done
    echo "scale=3; sqrt($sum_sq / $count)" | bc -l
}

wait_for_mount() {
    local mount_point=$1
    local timeout=${2:-30}
    local count=0
    while ! mountpoint -q "$mount_point" 2>/dev/null; do
        sleep 0.5
        count=$((count + 1))
        if [ $count -ge $((timeout * 2)) ]; then
            return 1
        fi
    done
    return 0
}

# Record a single test result to raw_results.csv
record_raw_result() {
    local label=$1
    local test=$2
    local run=$3
    local value=$4
    local unit=$5
    echo "$label,$test,$run,$value,$unit" >> "$RESULT_DIR/raw_results.csv"
}

# ==================== Dependency check ====================
check_dependencies() {
    log_info "Checking dependencies..."
    
    local missing=()
    
    [ -x "$CRYPTPILOT_VERITY" ] || command -v "$CRYPTPILOT_VERITY" >/dev/null 2>&1 || missing+=("cryptpilot-verity")
    command -v fio >/dev/null 2>&1 || missing+=("fio")
    command -v bc >/dev/null 2>&1 || missing+=("bc")
    command -v jq >/dev/null 2>&1 || missing+=("jq")
    command -v gocryptfs >/dev/null 2>&1 || missing+=("gocryptfs")
    command -v docker >/dev/null 2>&1 || missing+=("docker")
    
    if [ ${#missing[@]} -ne 0 ]; then
        log_error "Missing dependencies: ${missing[*]}"
        log_info "Install with: yum install -y gocryptfs fio bc jq docker"
        exit 1
    fi
    
    # Check root privileges (for drop_caches)
    if [ "$(id -u)" -ne 0 ]; then
        log_warn "Running as non-root, cannot drop caches, results may be inaccurate"
    fi
    
    # Check docker is running
    if ! docker info >/dev/null 2>&1; then
        log_error "Docker is not running. Please start docker service."
        exit 1
    fi
    
    # Pull cachefs image if not exists
    if ! docker image inspect "$CACHEFS_IMAGE" >/dev/null 2>&1; then
        log_info "Pulling cachefs image..."
        docker pull "$CACHEFS_IMAGE"
    fi
    
    log_success "Dependencies check passed"
}

# ==================== Test data preparation ====================
prepare_test_data() {
    log_info "Preparing test data..."
    
    mkdir -p "$TEST_DATA_DIR" "$TEST_MOUNT_POINT" "$RESULT_DIR" "$LOG_DIR"
    mkdir -p "$ENCRYPTED_DATA_DIR" "$VERITY_ENCRYPTED_DATA_DIR"
    mkdir -p "$CACHEFS_MOUNT_POINT" "$CACHEFS_VERITY_MOUNT_POINT" "$VERITY_ON_CACHEFS_MOUNT_POINT"
    
    # Check if test data already exists
    if [ -d "$TEST_DATA_DIR/large_files" ] && [ -f "$TEST_DATA_DIR/large_files/file_1.bin" ]; then
        log_info "Test data already exists, skipping generation"
        return
    fi
    
    log_info "Generating small files (${SMALL_FILE_COUNT} x 4KB)..."
    mkdir -p "$TEST_DATA_DIR/small_files"
    for i in $(seq 1 $SMALL_FILE_COUNT); do
        dd if=/dev/urandom of="$TEST_DATA_DIR/small_files/file_$i.bin" bs=4K count=1 2>/dev/null
    done
    
    log_info "Generating medium files (${MEDIUM_FILE_COUNT} x 1MB)..."
    mkdir -p "$TEST_DATA_DIR/medium_files"
    for i in $(seq 1 $MEDIUM_FILE_COUNT); do
        dd if=/dev/urandom of="$TEST_DATA_DIR/medium_files/file_$i.bin" bs=1M count=1 2>/dev/null
    done
    
    log_info "Generating large files (${LARGE_FILE_COUNT} x 100MB)..."
    mkdir -p "$TEST_DATA_DIR/large_files"
    for i in $(seq 1 $LARGE_FILE_COUNT); do
        dd if=/dev/urandom of="$TEST_DATA_DIR/large_files/file_$i.bin" bs=1M count=100 2>/dev/null
    done
    
    log_success "Test data preparation completed"
}

# ==================== gocryptfs encryption ====================
setup_gocryptfs_encrypted() {
    local source_dir=$1
    local encrypted_dir=$2
    local password=$3
    
    log_info "Initializing gocryptfs encryption for $encrypted_dir..."
    
    # Clean up existing encrypted directory
    rm -rf "$encrypted_dir"
    mkdir -p "$encrypted_dir"
    
    # Initialize gocryptfs
    echo "$password" | gocryptfs -init -q "$encrypted_dir" 2>"$LOG_DIR/gocryptfs_init.log"
    
    # Mount temporarily to copy files
    local temp_mount=$(mktemp -d)
    echo "$password" | gocryptfs -q "$encrypted_dir" "$temp_mount" 2>"$LOG_DIR/gocryptfs_mount.log"
    
    # Copy files
    log_info "Copying files to encrypted directory..."
    cp -r "$source_dir"/* "$temp_mount/"
    
    # Unmount
    fusermount -u "$temp_mount"
    rmdir "$temp_mount"
    
    log_success "gocryptfs encryption setup completed"
}

# ==================== verity-fuse setup ====================
# Format and mount verity-fuse (for fresh data)
setup_verity_fuse() {
    local data_dir=$1
    local mount_point=$2
    local label=$3
    
    log_info "Formatting verity metadata for $data_dir..."
    
    "$CRYPTPILOT_VERITY" format "$data_dir" --hash-output "$RESULT_DIR/${label}_root_hash.txt" --force \
        >> "$LOG_DIR/${label}_verity_format.log" 2>&1
    local root_hash=$(cat "$RESULT_DIR/${label}_root_hash.txt")
    
    log_info "Root Hash: $root_hash"
    mount_verity_fuse "$data_dir" "$mount_point" "$root_hash" "$label"
}

# Mount verity-fuse with existing root hash (no format)
mount_verity_fuse() {
    local data_dir=$1
    local mount_point=$2
    local root_hash=$3
    local label=$4
    
    log_info "Mounting verity-fuse at $mount_point..."
    
    # Run verity-fuse in background, redirect output to log file
    "$CRYPTPILOT_VERITY" open "$data_dir" "$mount_point" "$root_hash" \
        >> "$LOG_DIR/${label}_verity_open.log" 2>&1 &
    local pid=$!
    
    # Wait for mount
    if ! wait_for_mount "$mount_point" 30; then
        log_error "Timeout waiting for verity-fuse mount at $mount_point"
        kill $pid 2>/dev/null || true
        return 1
    fi
    
    log_success "verity-fuse mounted at $mount_point (PID: $pid)"
    echo $pid
}

teardown_verity_fuse() {
    local mount_point=$1
    log_info "Unmounting verity-fuse at $mount_point..."
    fusermount -u "$mount_point" 2>/dev/null || true
    sleep 1
}

# ==================== cachefs setup ====================
setup_cachefs() {
    local encrypted_dir=$1
    local mount_point=$2
    local password=$3
    local container_name=$4
    
    log_info "Starting cachefs container: $container_name..."
    
    # Create password file
    local passfile="$RESULT_DIR/${container_name}_passfile"
    echo -n "$password" > "$passfile"
    
    # Stop existing container if any
    docker rm -f "$container_name" 2>/dev/null || true
    
    # Start cachefs container
    docker run -d \
        --name "$container_name" \
        --cap-add SYS_ADMIN \
        --device /dev/fuse \
        -v "$encrypted_dir":/source:ro \
        -v "$mount_point":/target:shared \
        -v "$passfile":/passfile:ro \
        "$CACHEFS_IMAGE" \
        safe_cachefs.sh cache \
            --source-dir /source \
            --cache-dir memory \
            --cache-size 10240 \
            --encryption-mode gocryptfs \
            --passfile /passfile \
            cachefs /target \
        >> "$LOG_DIR/${container_name}.log" 2>&1
    
    # Wait for mount
    local timeout=60
    local count=0
    while ! mountpoint -q "$mount_point" 2>/dev/null; do
        sleep 1
        count=$((count + 1))
        if [ $count -ge $timeout ]; then
            log_error "Timeout waiting for cachefs mount at $mount_point"
            docker logs "$container_name" >> "$LOG_DIR/${container_name}.log" 2>&1
            return 1
        fi
    done
    
    log_success "cachefs mounted at $mount_point (container: $container_name)"
}

teardown_cachefs() {
    local mount_point=$1
    local container_name=$2
    
    log_info "Stopping cachefs container: $container_name..."
    docker rm -f "$container_name" 2>/dev/null || true
    
    # Ensure mount point is unmounted
    fusermount -u "$mount_point" 2>/dev/null || true
    sleep 1
}

# ==================== Single-run test functions ====================
# These functions run a single test iteration and record the raw result

test_sequential_read_dd_single() {
    local target_dir=$1
    local label=$2
    local run=$3
    local file="$target_dir/large_files/file_1.bin"
    
    drop_caches
    
    local output=$(dd if="$file" of=/dev/null bs=1M 2>&1)
    local speed=$(echo "$output" | grep -oP '[\d.]+\s*(MB|GB)/s' | head -1 | grep -oP '[\d.]+')
    local unit=$(echo "$output" | grep -oP '[\d.]+\s*(MB|GB)/s' | head -1 | grep -oP '(MB|GB)')
    
    if [ "$unit" = "GB" ]; then
        speed=$(echo "$speed * 1024" | bc -l)
    fi
    
    record_raw_result "$label" "sequential_read_dd" "$run" "$speed" "MB/s"
    log_info "  Run $run: sequential_read_dd = ${speed} MB/s"
}

test_sequential_read_fio_single() {
    local target_dir=$1
    local label=$2
    local run=$3
    
    drop_caches
    
    local output=$(fio --name=seq_read \
        --filename="$target_dir/large_files/file_1.bin" \
        --rw=read \
        --bs=4k \
        --direct=1 \
        --numjobs=1 \
        --time_based \
        --runtime=10 \
        --group_reporting \
        --output-format=json 2>/dev/null)
    
    local bw_kb=$(echo "$output" | jq -r '.jobs[0].read.bw')
    local bw_mb=$(echo "scale=3; $bw_kb / 1024" | bc -l)
    
    record_raw_result "$label" "sequential_read_fio" "$run" "$bw_mb" "MB/s"
    log_info "  Run $run: sequential_read_fio = ${bw_mb} MB/s"
}

test_random_read_fio_single() {
    local target_dir=$1
    local label=$2
    local run=$3
    
    drop_caches
    
    local output=$(fio --name=rand_read \
        --filename="$target_dir/large_files/file_1.bin" \
        --rw=randread \
        --bs=4k \
        --direct=1 \
        --numjobs=1 \
        --time_based \
        --runtime=10 \
        --group_reporting \
        --output-format=json 2>/dev/null)
    
    local iops=$(echo "$output" | jq -r '.jobs[0].read.iops')
    local lat_ns=$(echo "$output" | jq -r '.jobs[0].read.lat_ns.mean')
    local lat_ms=$(echo "scale=3; $lat_ns / 1000000" | bc -l)
    
    record_raw_result "$label" "random_read_iops" "$run" "$iops" "IOPS"
    record_raw_result "$label" "random_read_latency" "$run" "$lat_ms" "ms"
    log_info "  Run $run: random_read_iops = ${iops}, latency = ${lat_ms} ms"
}

test_small_files_read_single() {
    local target_dir=$1
    local label=$2
    local run=$3
    
    drop_caches
    
    local start=$(date +%s.%N)
    for i in $(seq 1 $SMALL_FILE_COUNT); do
        cat "$target_dir/small_files/file_$i.bin" > /dev/null
    done
    local end=$(date +%s.%N)
    
    local duration=$(echo "$end - $start" | bc -l)
    local ops_per_sec=$(echo "scale=3; $SMALL_FILE_COUNT / $duration" | bc -l)
    
    record_raw_result "$label" "small_files_read" "$run" "$duration" "seconds"
    record_raw_result "$label" "small_files_ops" "$run" "$ops_per_sec" "ops/s"
    log_info "  Run $run: small_files_read = ${duration} sec (${ops_per_sec} ops/s)"
}

test_readdir_single() {
    local target_dir=$1
    local label=$2
    local run=$3
    
    drop_caches
    
    local start=$(date +%s.%N)
    ls -laR "$target_dir" > /dev/null 2>&1
    local end=$(date +%s.%N)
    
    local duration=$(echo "($end - $start) * 1000" | bc -l)
    
    record_raw_result "$label" "readdir" "$run" "$duration" "ms"
    log_info "  Run $run: readdir = ${duration} ms"
}

# Run all single-iteration tests on a target directory
run_single_iteration_tests() {
    local target_dir=$1
    local label=$2
    local run=$3
    
    test_sequential_read_dd_single "$target_dir" "$label" "$run"
    test_sequential_read_fio_single "$target_dir" "$label" "$run"
    test_random_read_fio_single "$target_dir" "$label" "$run"
    test_small_files_read_single "$target_dir" "$label" "$run"
    test_readdir_single "$target_dir" "$label" "$run"
}

# Run a single test by name
run_single_test() {
    local target_dir=$1
    local label=$2
    local run=$3
    local test_name=$4
    
    case "$test_name" in
        sequential_read_dd)
            test_sequential_read_dd_single "$target_dir" "$label" "$run"
            ;;
        sequential_read_fio)
            test_sequential_read_fio_single "$target_dir" "$label" "$run"
            ;;
        random_read_fio)
            test_random_read_fio_single "$target_dir" "$label" "$run"
            ;;
        small_files_read)
            test_small_files_read_single "$target_dir" "$label" "$run"
            ;;
        readdir)
            test_readdir_single "$target_dir" "$label" "$run"
            ;;
        *)
            log_error "Unknown test: $test_name"
            return 1
            ;;
    esac
}

# List of test names
TEST_NAMES=("sequential_read_dd" "sequential_read_fio" "random_read_fio" "small_files_read" "readdir")

# ==================== Statistics calculation ====================
# Calculate avg/stddev from raw_results.csv and write to results.csv
calculate_statistics() {
    log_info "Calculating statistics from raw results..."
    
    echo "label,test,value,stddev,unit" > "$RESULT_DIR/results.csv"
    
    # Get unique label,test combinations
    local combinations=$(tail -n +2 "$RESULT_DIR/raw_results.csv" | cut -d',' -f1,2 | sort -u)
    
    while IFS=',' read -r label test; do
        # Get all values for this label,test
        local values=$(grep "^$label,$test," "$RESULT_DIR/raw_results.csv" | cut -d',' -f4)
        local unit=$(grep "^$label,$test," "$RESULT_DIR/raw_results.csv" | head -1 | cut -d',' -f5)
        
        # Convert to array
        local arr=()
        while IFS= read -r val; do
            arr+=("$val")
        done <<< "$values"
        
        # Calculate statistics
        local avg=$(calc_average "${arr[@]}")
        local stddev=$(calc_stddev "${arr[@]}")
        
        echo "$label,$test,$avg,$stddev,$unit" >> "$RESULT_DIR/results.csv"
        log_info "  $label,$test: avg=$avg, stddev=$stddev"
    done <<< "$combinations"
    
    log_success "Statistics calculated and saved to results.csv"
}

# ==================== Main test flow ====================
run_all_tests() {
    log_info "=========================================="
    log_info "Starting performance benchmark"
    log_info "  Runs per test: ${RUN_COUNT}"
    log_info "  Note: Full remount between each test to avoid cache effects"
    log_info "=========================================="
    
    # Initialize raw results file
    echo "label,test,run,value,unit" > "$RESULT_DIR/raw_results.csv"
    
    # Prepare encrypted data for cachefs tests (only once)
    log_info ""
    log_info ">>> Preparing encrypted data for cachefs tests <<<"
    setup_gocryptfs_encrypted "$TEST_DATA_DIR" "$ENCRYPTED_DATA_DIR" "$GOCRYPTFS_PASSWORD"
    
    # Prepare verity-formatted data for cachefs+verity tests
    local verity_source_dir="$BENCHMARK_DIR/verity_source_data"
    rm -rf "$verity_source_dir"
    cp -r "$TEST_DATA_DIR" "$verity_source_dir"
    
    log_info "Formatting source data with verity..."
    "$CRYPTPILOT_VERITY" format "$verity_source_dir" --hash-output "$RESULT_DIR/cachefs_verity_root_hash.txt" --force \
        >> "$LOG_DIR/cachefs_verity_format.log" 2>&1
    local verity_hash=$(cat "$RESULT_DIR/cachefs_verity_root_hash.txt")
    log_info "verity root hash: $verity_hash"
    
    setup_gocryptfs_encrypted "$verity_source_dir" "$VERITY_ENCRYPTED_DATA_DIR" "$GOCRYPTFS_PASSWORD"
    
    # Main test loop - RUN_COUNT iterations with full remount for each test
    for run in $(seq 1 $RUN_COUNT); do
        log_info ""
        log_info "=========================================="
        log_info ">>> Run $run of $RUN_COUNT <<<"
        log_info "=========================================="
        
        # 1. Baseline tests (direct access, no mount needed)
        # For baseline, no remount needed between tests since it's direct disk access
        log_info ""
        log_info ">>> Testing: baseline (Run $run) <<<"
        run_single_iteration_tests "$TEST_DATA_DIR" "baseline" "$run"
        
        # 2. verity-fuse tests - remount between each test to clear cache
        log_info ""
        log_info ">>> Testing: verity-fuse (Run $run) <<<"
        for test_name in "${TEST_NAMES[@]}"; do
            setup_verity_fuse "$TEST_DATA_DIR" "$TEST_MOUNT_POINT" "verity"
            run_single_test "$TEST_MOUNT_POINT" "verity-fuse" "$run" "$test_name"
            teardown_verity_fuse "$TEST_MOUNT_POINT"
        done
        
        # 3. cachefs tests - restart container between each test to clear memory cache
        log_info ""
        log_info ">>> Testing: cachefs (Run $run) <<<"
        for test_name in "${TEST_NAMES[@]}"; do
            setup_cachefs "$ENCRYPTED_DATA_DIR" "$CACHEFS_MOUNT_POINT" "$GOCRYPTFS_PASSWORD" "benchmark-cachefs"
            run_single_test "$CACHEFS_MOUNT_POINT" "cachefs" "$run" "$test_name"
            teardown_cachefs "$CACHEFS_MOUNT_POINT" "benchmark-cachefs"
        done
        
        # 4. cachefs + verity-fuse tests - restart both between each test
        log_info ""
        log_info ">>> Testing: cachefs+verity (Run $run) <<<"
        for test_name in "${TEST_NAMES[@]}"; do
            setup_cachefs "$VERITY_ENCRYPTED_DATA_DIR" "$CACHEFS_VERITY_MOUNT_POINT" "$GOCRYPTFS_PASSWORD" "benchmark-cachefs-verity"
            
            if ! mount_verity_fuse "$CACHEFS_VERITY_MOUNT_POINT" "$VERITY_ON_CACHEFS_MOUNT_POINT" "$verity_hash" "cachefs_verity"; then
                teardown_cachefs "$CACHEFS_VERITY_MOUNT_POINT" "benchmark-cachefs-verity"
                continue
            fi
            
            run_single_test "$VERITY_ON_CACHEFS_MOUNT_POINT" "cachefs+verity" "$run" "$test_name"
            teardown_verity_fuse "$VERITY_ON_CACHEFS_MOUNT_POINT"
            teardown_cachefs "$CACHEFS_VERITY_MOUNT_POINT" "benchmark-cachefs-verity"
        done
    done
    
    # Clean up verity source data
    rm -rf "$verity_source_dir"
    
    # Calculate statistics from raw results
    calculate_statistics
}

# ==================== Report generation ====================
generate_report() {
    log_info ""
    log_info "=========================================="
    log_info "Generating report"
    log_info "=========================================="
    
    local report_file="$RESULT_DIR/report.txt"
    local labels=("baseline" "verity-fuse" "cachefs" "cachefs+verity")
    
    {
        echo "cryptpilot-verity Performance Benchmark Report"
        echo "=============================================="
        echo "Test time: $(date)"
        echo "Runs per test: $RUN_COUNT (with full remount between each test)"
        echo ""
        echo "Test configuration:"
        echo "  - Small files: ${SMALL_FILE_COUNT} x 4KB"
        echo "  - Medium files: ${MEDIUM_FILE_COUNT} x 1MB"
        echo "  - Large files: ${LARGE_FILE_COUNT} x 100MB"
        echo ""
        echo "Test scenarios:"
        echo "  - baseline: Direct file access"
        echo "  - verity-fuse: verity-fuse only"
        echo "  - cachefs: gocryptfs encryption + cachefs decryption"
        echo "  - cachefs+verity: gocryptfs + cachefs + verity-fuse"
        echo ""
        echo "Results (change % vs baseline, + is better, - is worse):"
        echo ""
        
        # Header
        printf "| %-28s |" "Test"
        for label in "${labels[@]}"; do
            printf " %-18s |" "$label"
        done
        echo ""
        
        # Separator
        printf "|%s|" "$(printf -- '-%.0s' {1..30})"
        for label in "${labels[@]}"; do
            printf "%s|" "$(printf -- '-%.0s' {1..20})"
        done
        echo ""
        
        # Data rows - test name with unit
        declare -A test_units=(
            ["sequential_read_dd"]="MB/s"
            ["sequential_read_fio"]="MB/s"
            ["random_read_iops"]="IOPS"
            ["random_read_latency"]="ms"
            ["small_files_read"]="seconds"
            ["small_files_ops"]="ops/s"
            ["readdir"]="ms"
        )
        
        # Tests where lower is better (latency, time)
        declare -A lower_is_better=(
            ["random_read_latency"]=1
            ["small_files_read"]=1
            ["readdir"]=1
        )
        
        for test in "sequential_read_dd" "sequential_read_fio" "random_read_iops" "random_read_latency" "small_files_read" "readdir"; do
            local unit="${test_units[$test]}"
            printf "| %-28s |" "$test ($unit)"
            local baseline_val=""
            for label in "${labels[@]}"; do
                local val=$(grep "^$label,$test," "$RESULT_DIR/results.csv" 2>/dev/null | cut -d',' -f3)
                if [ -n "$val" ]; then
                    if [ "$label" = "baseline" ]; then
                        baseline_val="$val"
                        printf " %-18s |" "$val"
                    else
                        # Calculate change percentage vs baseline
                        if [ -n "$baseline_val" ] && [ "$baseline_val" != "0" ]; then
                            # Ensure numbers have leading zero for bc
                            local safe_val=$(echo "$val" | sed 's/^\./0./')
                            local safe_baseline=$(echo "$baseline_val" | sed 's/^\./0./')
                            
                            # Calculate change: (val - baseline) / baseline * 100
                            # For lower_is_better metrics, invert the sign
                            local change
                            if [ -n "${lower_is_better[$test]}" ]; then
                                # Lower is better: show as positive if value decreased (improvement)
                                change=$(echo "scale=3; ($safe_baseline - $safe_val) / $safe_baseline * 100" | bc -l 2>/dev/null | xargs printf "%.1f" || echo "0")
                            else
                                # Higher is better: positive change means improvement
                                change=$(echo "scale=3; ($safe_val - $safe_baseline) / $safe_baseline * 100" | bc -l 2>/dev/null | xargs printf "%.1f" || echo "0")
                            fi
                            
                            # Format with +/- sign
                            local sign=""
                            # Check if change is positive (starts with digit or .)
                            if [[ "$change" =~ ^[0-9.] ]]; then
                                sign="+"
                            fi
                            # If change starts with -, sign is already there
                            
                            printf " %-10s (%s%s%%) |" "$val" "$sign" "$change"
                        else
                            printf " %-18s |" "$val"
                        fi
                    fi
                else
                    printf " %-18s |" "N/A"
                fi
            done
            echo ""
        done
        
    } > "$report_file"
    
    cat "$report_file"
    
    log_success "Report saved to: $report_file"
    log_success "CSV data saved to: $RESULT_DIR/results.csv"
    log_success "Raw results saved to: $RESULT_DIR/raw_results.csv"
    log_success "Logs saved to: $LOG_DIR/"
}

# ==================== Cleanup ====================
cleanup() {
    log_info "Cleaning up test environment..."
    
    # Stop all containers
    docker rm -f benchmark-cachefs benchmark-cachefs-verity 2>/dev/null || true
    
    # Unmount all mount points
    fusermount -u "$TEST_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$CACHEFS_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$CACHEFS_VERITY_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$VERITY_ON_CACHEFS_MOUNT_POINT" 2>/dev/null || true
    
    read -p "Delete test data? [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf "$TEST_DATA_DIR" "$ENCRYPTED_DATA_DIR" "$VERITY_ENCRYPTED_DATA_DIR"
        rm -rf "$BENCHMARK_DIR/verity_source_data"
        log_success "Test data deleted"
    fi
}

cleanup_all_mounts() {
    docker rm -f benchmark-cachefs benchmark-cachefs-verity 2>/dev/null || true
    fusermount -u "$TEST_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$CACHEFS_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$CACHEFS_VERITY_MOUNT_POINT" 2>/dev/null || true
    fusermount -u "$VERITY_ON_CACHEFS_MOUNT_POINT" 2>/dev/null || true
}

# ==================== Help ====================
show_help() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --prepare     Prepare test data only"
    echo "  --run         Run benchmark (requires prepared data)"
    echo "  --cleanup     Cleanup test environment"
    echo "  --all         Prepare data, run benchmark, generate report (default)"
    echo "  --help        Show this help"
    echo ""
    echo "Test scenarios:"
    echo "  - baseline:       Direct file access"
    echo "  - verity-fuse:    verity-fuse integrity verification"
    echo "  - cachefs:        gocryptfs + cachefs decryption"
    echo "  - cachefs+verity: gocryptfs + cachefs + verity-fuse"
    echo ""
    echo "Environment variables:"
    echo "  TEST_DATA_DIR     Test data directory"
    echo "  RESULT_DIR        Results output directory"
    echo "  LOG_DIR           Logs output directory"
}

# ==================== Main ====================
main() {
    case "${1:---all}" in
        --prepare)
            check_dependencies
            prepare_test_data
            ;;
        --run)
            check_dependencies
            run_all_tests
            generate_report
            ;;
        --cleanup)
            cleanup
            ;;
        --all)
            check_dependencies
            prepare_test_data
            run_all_tests
            generate_report
            ;;
        --help|-h)
            show_help
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
}

# Trap exit signal for cleanup
trap 'cleanup_all_mounts 2>/dev/null' EXIT

main "$@"
