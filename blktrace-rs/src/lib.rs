//! Linux block-layer I/O trace capture, straight from the kernel.
//!
//! `blktrace_rs` drives the kernel block tracing interface directly (the
//! `BLKTRACESETUP`/`BLKTRACESTART`/`BLKTRACESTOP`/`BLKTRACETEARDOWN` ioctls
//! plus the per-CPU relay channels under debugfs), so it needs neither the
//! `blktrace` nor the `blkparse` command line tools.
//!
//! Capturing requires root and a kernel with debugfs. Building this crate
//! requires `libclang` and the kernel UAPI headers
//! (`linux/blktrace_api.h`).
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
//!         println!("read {} bytes at sector {}", event.event.bytes, event.event.sector);
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
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(not(target_os = "linux"))]
compile_error!("blktrace-rs only supports Linux");

#[cfg(feature = "async")]
mod async_api;
mod debugfs;
mod error;
mod event;
mod session;
mod uapi;

pub use uapi::*;

pub use error::BlktraceError;
pub use event::BlkTraceEvent;
pub use session::{BlkTrace, BlkTraceReport, BlkTraceSession};
