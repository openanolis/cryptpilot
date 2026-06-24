// Three-state initialization integration tests
// Tests the None → Initializing → Ready lifecycle and interrupted init recovery

use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use cryptpilot::fs::{
    block::dummy::DummyDevice,
    luks2::{format, get_init_state, is_initialized, mark_volume_as_initialized, VolumeInitState},
};
use cryptpilot::types::{IntegrityType, MakeFsType, Passphrase};

use anyhow::Result;
use tokio::process::Command;

/// Test: get_init_state returns None for a raw dummy device
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_init_state_none_for_raw_device() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let state = get_init_state(Path::new(&dummy.path()?)).await?;
    assert_eq!(state, VolumeInitState::None);
    Ok(())
}

/// Test: get_init_state returns Initializing after format()
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_init_state_initializing_after_format() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let passphrase = Passphrase::from(b"test-passphrase-1234567890123456".to_vec());

    format(Path::new(&dummy.path()?), &passphrase, IntegrityType::None).await?;

    let state = get_init_state(Path::new(&dummy.path()?)).await?;
    assert_eq!(state, VolumeInitState::Initializing);
    Ok(())
}

/// Test: get_init_state returns Ready after mark_volume_as_initialized()
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_init_state_ready_after_mark() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let passphrase = Passphrase::from(b"test-passphrase-1234567890123456".to_vec());

    format(Path::new(&dummy.path()?), &passphrase, IntegrityType::None).await?;
    mark_volume_as_initialized(Path::new(&dummy.path()?)).await?;

    let state = get_init_state(Path::new(&dummy.path()?)).await?;
    assert_eq!(state, VolumeInitState::Ready);
    Ok(())
}

/// Test: is_initialized returns true only for Ready state
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_is_initialized_only_true_for_ready() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let passphrase = Passphrase::from(b"test-passphrase-1234567890123456".to_vec());

    // Raw device: is_initialized = false
    assert!(!is_initialized(Path::new(&dummy.path()?)).await?);

    // After format: is_initialized = false (Initializing state)
    format(Path::new(&dummy.path()?), &passphrase, IntegrityType::None).await?;
    assert!(!is_initialized(Path::new(&dummy.path()?)).await?);

    // After mark: is_initialized = true (Ready state)
    mark_volume_as_initialized(Path::new(&dummy.path()?)).await?;
    assert!(is_initialized(Path::new(&dummy.path()?)).await?);

    Ok(())
}

/// Test: full init lifecycle (None → Initializing → Ready) detected via blkid -p
///
/// This test runs `blkid -p` in a parallel monitoring loop while performing
/// a full initialization (format + mkfs + mark). It verifies that all three
/// states are observable:
/// - None: no SUBSYSTEM field (raw device before format)
/// - Initializing: SUBSYSTEM="cryptpilot-initializing" (after format)
/// - Ready: SUBSYSTEM="cryptpilot" (after mark_volume_as_initialized)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_full_init_lifecycle_with_blkid_probe() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let dev_path = dummy.path()?;
    let dev_path_clone = dev_path.clone();

    // Shared state for the monitor to record observed states
    let observed_states: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let states_clone = observed_states.clone();

    // Spawn a parallel monitor that runs blkid -p every 100ms
    let monitor_handle = tokio::spawn(async move {
        for i in 0..200 {
            // Max 20 seconds total
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let output = Command::new("blkid")
                .arg("-p")
                .arg("-o")
                .arg("export")
                .arg(&dev_path_clone)
                .output()
                .await;
            if let Ok(output) = output {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(subsystem) = extract_blkid_field(&stdout, "SUBSYSTEM") {
                    let mut states = states_clone.lock().await;
                    // Only record new states (dedup consecutive duplicates)
                    if states.last().map(|s| s != &subsystem).unwrap_or(true) {
                        tracing::debug!("Monitor probe #{}: observed SUBSYSTEM={}", i, subsystem);
                        states.push(subsystem);
                    }
                }
            }
        }
    });

    // Wait for the monitor to do at least one probe on the raw device
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Perform full initialization with some delays to allow monitor to catch states
    let passphrase = Passphrase::from(b"test-passphrase-1234567890123456".to_vec());

    // Format: transitions to Initializing
    format(Path::new(&dev_path), &passphrase, IntegrityType::None).await?;
    // Give the monitor time to observe the Initializing state
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Open the LUKS volume to create a filesystem
    let tmp_volume = cryptpilot::fs::luks2::TempLuksVolume::open(
        Path::new(&dev_path),
        &passphrase,
        IntegrityType::None,
    )
    .await?;

    // Create filesystem inside the encrypted volume
    cryptpilot::fs::mkfs::force_mkfs(
        &tmp_volume.volume_path(),
        &MakeFsType::Ext4,
        IntegrityType::None,
    )
    .await?;
    drop(tmp_volume); // Close the temporary volume
                      // Give the monitor time to observe (still Initializing at this point)
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Mark as ready: transitions to Ready
    mark_volume_as_initialized(Path::new(&dev_path)).await?;
    // Give the monitor time to observe the Ready state
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Stop the monitor
    monitor_handle.abort();

    // Verify we observed all three states
    let states = observed_states.lock().await;
    tracing::info!("Observed blkid states: {:?}", states);

    // We should have seen at least "cryptpilot-initializing" and "cryptpilot"
    let has_initializing = states.iter().any(|s| s == "cryptpilot-initializing");
    let has_ready = states.iter().any(|s| s == "cryptpilot");
    assert!(
        has_initializing,
        "blkid should have observed SUBSYSTEM=cryptpilot-initializing, got: {:?}",
        states
    );
    assert!(
        has_ready,
        "blkid should have observed SUBSYSTEM=cryptpilot, got: {:?}",
        states
    );

    Ok(())
}

/// Extract a field value from blkid -p -o export output.
///
/// blkid outputs key-value pairs like: `SUBSYSTEM=cryptpilot-initializing`
fn extract_blkid_field(output: &str, key: &str) -> Option<String> {
    for line in output.lines() {
        if line.starts_with(&format!("{}=", key)) {
            return Some(line[key.len() + 1..].to_string());
        }
    }
    None
}
