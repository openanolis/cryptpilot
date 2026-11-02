//! Linux block-layer I/O trace capture, straight from the kernel.
//!
//! `blktrace_rs` drives the kernel block tracing interface directly (the
//! `BLKTRACESETUP2`/`BLKTRACESETUP`/`BLKTRACESTART`/`BLKTRACESTOP`/
//! `BLKTRACETEARDOWN` ioctls plus the per-CPU relay channels under
//! debugfs), so it needs neither the `blktrace` nor the `blkparse`
//! command line tools.
//!
//! Sessions prefer the v2 trace API, which since Linux 6.19 is the only
//! format that records write-zeroes requests (they got their own trace
//! category, above the v1 ceiling, in commit bc49af56eea8; v1 sessions
//! silently drop such records). On older kernels the setup falls back to
//! the v1 API automatically, where write-zeroes requests are recorded as
//! ordinary writes; events are normalized into [`BlkTraceEvent`] either
//! way.
//!
//! Capturing requires root and a kernel with debugfs. Building needs
//! neither `libclang` nor kernel UAPI headers: every UAPI layout is
//! hand-written and pinned to upstream header commits.
//!
//! # Example
//! ```no_run
//! use blktrace_rs::BlkTrace;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let session = BlkTrace::monitor("/dev/sda")?;
//! // ... do I/O on the device while the tracer runs ...
//! let report = session.shutdown()?;
//! for event in &report.events {
//!     if event.is_read() {
//!         println!("read {} bytes at sector {}", event.bytes, event.sector);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # The `async` feature
//! The tracing core is inherently blocking (relay channel reads and ioctl
//! waits). With `features = ["async"]` the crate adds `monitor_async` and
//! `shutdown_async`, which run that core on the tokio blocking pool so
//! async callers never stall their executor.

#![warn(missing_docs)]
// On docs.rs builds this shows feature-gated items (the async API) with
// cfg badges instead of hiding them.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(target_os = "linux"))]
compile_error!("blktrace-rs only supports Linux");

#[cfg(feature = "async")]
mod async_api;
mod debugfs;
mod error;
mod event;
mod session;
mod uapi;

pub use error::BlktraceError;
pub use event::{BlkIoKind, BlkTraceEvent};
pub use session::{BlkTrace, BlkTraceReport, BlkTraceSession};
