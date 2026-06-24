// Three-state initialization integration tests
// Tests the None → Initializing → Ready lifecycle and interrupted init recovery

use std::path::Path;

use cryptpilot::fs::{
    block::dummy::DummyDevice,
    luks2::{format, get_init_state, is_initialized, mark_volume_as_initialized, VolumeInitState},
};
use cryptpilot::types::{IntegrityType, Passphrase};

use anyhow::Result;

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
