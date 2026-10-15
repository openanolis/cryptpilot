//! Live capture against a loop device. Requires root, loop device support
//! and debugfs; skips itself otherwise.

use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::Command,
};

use blktrace_rs::BlkTrace;

/// uid via /proc metadata to avoid a test-only dependency;
/// /proc/self is owned by the process owner.
fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|meta| meta.uid() == 0)
        .unwrap_or(false)
}

/// Owns an attached loop device and its backing file. Dropping detaches
/// and deletes both, so a panic past setup cannot strand the device or
/// the 64 MiB file in tmp.
struct LoopDevice {
    device: PathBuf,
    backing: PathBuf,
}

impl Drop for LoopDevice {
    // The workspace disallowed_methods lint points Command users at
    // cryptpilot-core's CheckCommandOutput, an async tokio API this
    // standalone crate cannot use; cleanup here is best-effort.
    #[allow(clippy::disallowed_methods)]
    fn drop(&mut self) {
        let _ = Command::new("losetup").arg("-d").arg(&self.device).status();
        let _ = std::fs::remove_file(&self.backing);
    }
}

/// Attaches a sparse backing file to a loop device; None when loop
/// devices are unavailable (containers without loop support). `tag` keeps
/// the backing file unique when several tests share one process. Each
/// failure path below cleans up whatever it created before returning;
/// once the guard exists, Drop owns the teardown.
// Same lint exemption as the Drop impl above; the exit status is checked
// by hand here.
#[allow(clippy::disallowed_methods)]
fn setup_loop_device(tag: &str) -> Option<LoopDevice> {
    let backing =
        std::env::temp_dir().join(format!("blktrace-rs-test-{tag}-{}", std::process::id()));
    let file = std::fs::File::create(&backing).ok()?;
    if file.set_len(64 * 1024 * 1024).is_err() {
        std::fs::remove_file(&backing).ok();
        return None;
    }
    drop(file);

    let Ok(output) = Command::new("losetup")
        .args(["-f", "--show"])
        .arg(&backing)
        .output()
    else {
        std::fs::remove_file(&backing).ok();
        return None;
    };
    if !output.status.success() {
        std::fs::remove_file(&backing).ok();
        return None;
    }
    let Ok(loop_dev) = String::from_utf8(output.stdout) else {
        // The attach succeeded but the name is undecodable; losetup also
        // accepts the backing file as the detach target.
        let _ = Command::new("losetup").arg("-d").arg(&backing).status();
        std::fs::remove_file(&backing).ok();
        return None;
    };
    let loop_dev = loop_dev.trim().to_owned();
    eprintln!("using loop device {loop_dev}");
    Some(LoopDevice {
        device: PathBuf::from(loop_dev),
        backing,
    })
}

#[test]
fn captures_reads_from_a_loop_device() {
    if !is_root() {
        eprintln!("skipping: not root");
        return;
    }
    let Some(loop_dev) = setup_loop_device("sync") else {
        eprintln!("skipping: no loop device available");
        return;
    };

    let session = BlkTrace::monitor(&loop_dev.device).expect("monitor starts");
    // Reads served from the page cache never reach the block layer.
    BlkTrace::drop_page_cache().expect("root can drop the page cache");

    {
        let mut reader = OpenOptions::new()
            .read(true)
            .open(&loop_dev.device)
            .expect("open loop device");
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut [0; 125]).unwrap();
        reader.seek(SeekFrom::Start(4096)).unwrap();
        reader.read_exact(&mut [0; 1025]).unwrap();
    }

    let report = session.shutdown().expect("shutdown collects the report");

    assert_eq!(report.dropped, 0);
    assert!(!report.events.is_empty());
    let count_read = report.events.iter().filter(|event| event.is_read()).count();
    assert!(
        count_read > 0,
        "expected read events, got {:?}",
        report.events
    );
}

#[cfg(feature = "async")]
mod async_variant {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn captures_reads_from_a_loop_device_async() {
        if !is_root() {
            eprintln!("skipping: not root");
            return;
        }
        // The async test shares the process with the sync one; the tag
        // keeps their backing files apart.
        let Some(loop_dev) = setup_loop_device("async") else {
            eprintln!("skipping: no loop device available");
            return;
        };

        let session = BlkTrace::monitor_async(&loop_dev.device)
            .await
            .expect("monitor starts");
        BlkTrace::drop_page_cache().expect("root can drop the page cache");

        {
            let mut reader = OpenOptions::new()
                .read(true)
                .open(&loop_dev.device)
                .expect("open loop device");
            reader.seek(SeekFrom::Start(0)).unwrap();
            reader.read_exact(&mut [0; 125]).unwrap();
            reader.seek(SeekFrom::Start(4096)).unwrap();
            reader.read_exact(&mut [0; 1025]).unwrap();
        }

        let report = session
            .shutdown_async()
            .await
            .expect("shutdown collects the report");

        assert_eq!(report.dropped, 0);
        assert!(!report.events.is_empty());
        let count_read = report.events.iter().filter(|event| event.is_read()).count();
        assert!(
            count_read > 0,
            "expected read events, got {:?}",
            report.events
        );
    }
}
