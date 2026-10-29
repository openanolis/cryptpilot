//! Making sure debugfs is available: the kernel exposes the blktrace relay
//! channels under it.

use std::path::Path;

use nix::mount::{mount, MsFlags};

use crate::error::BlktraceError;

const DEBUGFS: &str = "/sys/kernel/debug";

/// Errors from scanning /proc/self/mountinfo are tolerated: a stale or
/// unreadable mount list falls through to a mount attempt, which fails
/// loudly if debugfs really is unavailable.
pub(crate) fn ensure_debugfs() -> Result<(), BlktraceError> {
    let debugfs = Path::new(DEBUGFS);
    if !debugfs.exists() {
        return Err(BlktraceError::DebugfsUnsupported);
    }

    // mnt's ParseError has no conversion into BlktraceError, so a failed
    // mountinfo scan is treated as "not mounted" and falls through, as the
    // doc comment above specifies.
    let mounted = mnt::MountIter::new_from_proc()
        .map(|mut iter| iter.any(|item| item.map(|item| item.file == debugfs).unwrap_or(false)))
        .unwrap_or(false);
    if mounted {
        return Ok(());
    }

    tracing::debug!("debugfs not mounted, mounting it now");
    std::fs::create_dir_all(debugfs)?;
    mount(
        Some("debugfs"),
        debugfs,
        Some("debugfs"),
        MsFlags::empty(),
        Option::<&str>::None,
    )
    .map_err(BlktraceError::MountDebugfs)?;

    Ok(())
}

/// Drops the page cache.
///
/// Reads served from the page cache never reach the block layer, so a
/// caller that wants read events in its trace drops the cache first, right
/// after starting the session and before doing the reads it cares about.
pub fn drop_page_cache() -> std::io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open("/proc/sys/vm/drop_caches")?;
    file.write_all(b"1")
}

#[cfg(test)]
mod tests {
    use super::*;

    // uid via /proc metadata to avoid pulling a nix feature just for a test
    // guard; /proc/self is owned by the process owner.
    fn is_root() -> bool {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self")
            .map(|meta| meta.uid() == 0)
            .unwrap_or(false)
    }

    #[test]
    fn ensure_debugfs_succeeds_for_root() {
        if !is_root() {
            return;
        }
        ensure_debugfs().expect("debugfs is available on a root test machine");
    }

    #[test]
    fn drop_page_cache_writes_when_root() {
        if !is_root() {
            return;
        }
        drop_page_cache().expect("root can write to /proc/sys/vm/drop_caches");
    }
}
