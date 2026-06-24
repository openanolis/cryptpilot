// Three-state initialization integration tests
// Tests the None → Initializing → Ready lifecycle and interrupted init recovery

use std::path::Path;

use cryptpilot::fs::{
    block::dummy::DummyDevice,
    cmd::CheckCommandOutput as _,
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

/// Test: full init lifecycle (None → Initializing → Ready) with blkid probing
///
/// Performs a full initialization (format + mkfs + mark). After each step,
/// verifies the state via `get_init_state()` AND checks that `blkid -p`
/// reports the correct SUBSYSTEM field.
///
/// Uses `serial_test` to avoid interference from other parallel tests.
#[serial_test::serial]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_full_init_lifecycle_with_blkid_probe() -> Result<()> {
    let dummy = DummyDevice::setup_on_tmpfs(100 * 1024 * 1024).await?;
    let dev_path = dummy.path()?;

    let passphrase = Passphrase::from(b"test-passphrase-1234567890123456".to_vec());

    // Step 1: Verify raw device is None
    let state = get_init_state(Path::new(&dev_path)).await?;
    assert_eq!(state, VolumeInitState::None, "Expected None before format");

    // Step 2: Format → Initializing
    format(Path::new(&dev_path), &passphrase, IntegrityType::None).await?;
    let state = get_init_state(Path::new(&dev_path)).await?;
    assert_eq!(
        state,
        VolumeInitState::Initializing,
        "Expected Initializing after format"
    );
    // Wait and retry blkid until it sees the state (up to 3s)
    wait_for_blkid_subsystem(dev_path.to_str().unwrap(), "cryptpilot-initializing", 30).await?;

    // Step 3: Open LUKS + mkfs
    let tmp_volume = cryptpilot::fs::luks2::TempLuksVolume::open(
        Path::new(&dev_path),
        &passphrase,
        IntegrityType::None,
    )
    .await?;
    cryptpilot::fs::mkfs::force_mkfs(
        &tmp_volume.volume_path(),
        &MakeFsType::Ext4,
        IntegrityType::None,
    )
    .await?;
    drop(tmp_volume);

    // Still Initializing after mkfs
    let state = get_init_state(Path::new(&dev_path)).await?;
    assert_eq!(
        state,
        VolumeInitState::Initializing,
        "Expected Initializing after mkfs"
    );

    // Step 4: Mark as ready → Ready
    mark_volume_as_initialized(Path::new(&dev_path)).await?;
    let state = get_init_state(Path::new(&dev_path)).await?;
    assert_eq!(state, VolumeInitState::Ready, "Expected Ready after mark");
    // Wait and retry blkid until it sees the state (up to 5s)
    wait_for_blkid_subsystem(dev_path.to_str().unwrap(), "cryptpilot", 50).await?;

    Ok(())
}

/// Poll blkid until it reports the expected SUBSYSTEM value.
///
/// Retries `max_attempts` times with 100ms between attempts.
/// Returns Ok if the expected value is observed, Err if not.
async fn wait_for_blkid_subsystem(
    dev_path: &str,
    expected: &str,
    max_attempts: usize,
) -> Result<()> {
    for attempt in 1..=max_attempts {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let result = Command::new("blkid")
            .arg("-p")
            .arg("-c")
            .arg("/dev/null")
            .arg("-o")
            .arg("export")
            .arg(dev_path)
            .run_with_status_checker(|code, stdout, stderr| {
                // blkid -p exits with 2 if no signature found, 0 if found
                if code == 0 || code == 2 {
                    Ok(String::from_utf8_lossy(&stdout).to_string())
                } else {
                    Err(anyhow::anyhow!(
                        "blkid failed with code {}: stderr={}",
                        code,
                        String::from_utf8_lossy(&stderr)
                    ))
                }
            })
            .await;
        if let Ok(stdout) = result {
            if let Some(subsystem) = extract_blkid_field(&stdout, "SUBSYSTEM") {
                if subsystem == expected {
                    return Ok(());
                }
            }
        }
        if attempt == max_attempts {
            anyhow::bail!(
                "blkid did not report SUBSYSTEM={} after {} attempts ({}ms)",
                expected,
                max_attempts,
                max_attempts * 100
            );
        }
    }
    Ok(())
}

/// Extract a field value from blkid -p -o export output.
fn extract_blkid_field(output: &str, key: &str) -> Option<String> {
    for line in output.lines() {
        if line.starts_with(&format!("{}=", key)) {
            return Some(line[key.len() + 1..].to_string());
        }
    }
    None
}
