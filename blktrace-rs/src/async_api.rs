//! Async wrappers around the blocking core, behind the `async` feature.
//!
//! The tracing core is inherently blocking (relay channel reads, ioctl
//! waits, a deliberate flush wait). These wrappers push all of it onto the
//! tokio blocking pool, so async callers never stall their executor.

use std::path::Path;

use tokio::task::spawn_blocking;

use crate::error::BlktraceError;
use crate::session::{BlkTrace, BlkTraceReport, BlkTraceSession};

impl BlkTrace {
    /// Async version of [`BlkTrace::monitor`].
    pub async fn monitor_async(path: impl AsRef<Path>) -> Result<BlkTraceSession, BlktraceError> {
        let path = path.as_ref().to_owned();
        spawn_blocking(move || BlkTrace::monitor(path))
            .await
            .map_err(|_| BlktraceError::ThreadPanicked)?
    }
}

impl BlkTraceSession {
    /// Async version of [`BlkTraceSession::shutdown`].
    pub async fn shutdown_async(self) -> Result<BlkTraceReport, BlktraceError> {
        spawn_blocking(move || self.shutdown())
            .await
            .map_err(|_| BlktraceError::ThreadPanicked)?
    }
}
