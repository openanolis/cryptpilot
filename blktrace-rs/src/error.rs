use std::io;

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum BlktraceError {
    /// The kernel has no debugfs support, which blktrace requires.
    #[error("debugfs (/sys/kernel/debug) is not supported by the current kernel")]
    DebugfsUnsupported,
    /// Mounting debugfs failed.
    #[error("failed to mount debugfs")]
    MountDebugfs(#[source] nix::Error),
    /// The `BLKTRACESETUP` ioctl failed.
    #[error("BLKTRACESETUP failed")]
    Setup(#[source] nix::Error),
    /// The `BLKTRACESTART` ioctl failed.
    #[error("BLKTRACESTART failed")]
    Start(#[source] nix::Error),
    /// The `BLKTRACESTOP` ioctl failed.
    #[error("BLKTRACESTOP failed")]
    Stop(#[source] nix::Error),
    /// The `BLKTRACETEARDOWN` ioctl failed.
    #[error("BLKTRACETEARDOWN failed")]
    Teardown(#[source] nix::Error),
    /// The `BLKFLSBUF` ioctl failed.
    #[error("BLKFLSBUF failed")]
    FlushBuffers(#[source] nix::Error),
    /// A trace record's magic number did not match the kernel UAPI value.
    #[error("blktrace event magic number mismatch")]
    MagicMismatch,
    /// The block name the kernel returned is not valid UTF-8.
    #[error("invalid block name returned by the kernel")]
    InvalidBlockName,
    /// Reading the dropped-event counter failed.
    #[error("failed to read the dropped events count")]
    ReadDropped(#[source] io::Error),
    /// The dropped-event counter is not a number.
    #[error("failed to parse the dropped events count")]
    ParseDropped,
    /// A reader or collector thread died unexpectedly.
    #[error("a tracing thread panicked")]
    ThreadPanicked,
    /// An I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}
