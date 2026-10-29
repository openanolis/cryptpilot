//! The tracing session: kernel setup, per-CPU reader threads, teardown.

use std::{
    ffi::CStr,
    fs::File,
    os::fd::{AsFd, AsRawFd},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

use crate::debugfs::ensure_debugfs;
use crate::error::BlktraceError;
use crate::event::{read_event, BlkTraceEvent};
use crate::uapi::{
    blk_user_trace_setup, blktrace_cat_BLK_TC_READ, blktrace_cat_BLK_TC_WRITE, ioctl,
};

/// Size of each relay buffer the kernel allocates, per CPU.
const BLK_TRACE_BUF_SIZE: u32 = 4 * 1024 * 1024;
/// Number of relay buffers the kernel keeps spare, per CPU.
const BLK_TRACE_BUF_COUNT: u32 = 16;
/// How long a reader waits before re-checking a relay channel that is at
/// EOF; the kernel appends records as the traced I/O happens.
const RETRY_INTERVAL: Duration = Duration::from_millis(10);
/// How long shutdown waits for the kernel to flush pending events into the
/// relay channels before draining them.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// Entry point for starting a trace.
pub struct BlkTrace;

impl BlkTrace {
    /// Starts tracing the block device at `path`.
    ///
    /// Only read and write events are captured. Reads served from the page
    /// cache never reach the block layer, so call
    /// [`BlkTrace::drop_page_cache`] before the reads you want traced.
    /// Stopping with [`BlkTraceSession::shutdown`] returns every event
    /// recorded in between; dropping the session instead stops tracing but
    /// discards the events.
    pub fn monitor(path: impl AsRef<Path>) -> Result<BlkTraceSession, BlktraceError> {
        BlkTraceSession::start(path.as_ref())
    }

    /// Drops the page cache; see the module docs on
    /// [`BlkTrace::monitor`] for when this matters.
    pub fn drop_page_cache() -> std::io::Result<()> {
        crate::debugfs::drop_page_cache()
    }
}

/// The outcome of a completed tracing session.
pub struct BlkTraceReport {
    /// All events captured, in arrival order across CPUs.
    pub events: Vec<BlkTraceEvent>,
    /// Events the kernel dropped because a relay buffer overflowed.
    /// Non-zero means the recording is incomplete.
    pub dropped: u64,
}

/// A live tracing session.
///
/// Created by [`BlkTrace::monitor`]. Call [`BlkTraceSession::shutdown`] to
/// stop tracing and collect the events; dropping it stops the kernel tracer
/// but discards the events.
pub struct BlkTraceSession {
    device: File,
    block_name: String,
    cancel: Arc<AtomicBool>,
    // `start` builds the session as the spawns succeed, so a spawn failure
    // after BLKTRACESTART unwinds into `Drop`, which cancels and joins the
    // threads here before stopping the kernel tracer.
    readers: Vec<thread::JoinHandle<Result<(), BlktraceError>>>,
    // Option so `shutdown` can move the handle out: this type implements
    // Drop (kernel STOP/TEARDOWN), so fields cannot be moved out directly.
    collector: Option<thread::JoinHandle<Vec<BlkTraceEvent>>>,
}

impl BlkTraceSession {
    fn start(path: &Path) -> Result<Self, BlktraceError> {
        ensure_debugfs()?;

        let device = File::open(path)?;
        let fd = device.as_fd();

        let mut setup_data = blk_user_trace_setup {
            // Capture read and write events only
            // See: https://github.com/sdsc/blktrace/blob/dd093eb1c48e0d86b835758b96a9886fb7773aa4/act_mask.c#L40
            act_mask: (blktrace_cat_BLK_TC_READ | blktrace_cat_BLK_TC_WRITE) as u16,
            buf_size: BLK_TRACE_BUF_SIZE,
            buf_nr: BLK_TRACE_BUF_COUNT,
            ..Default::default()
        };
        // SAFETY: fd is an open block device fd; setup_data is a valid,
        // fully initialized blk_user_trace_setup.
        unsafe { ioctl::blktrace_setup(fd.as_raw_fd(), &mut setup_data) }
            .map_err(BlktraceError::Setup)?;

        // The kernel returns the name of the block device it registered
        // the tracer under; relay channels and counters hang off it.
        // SAFETY: `name` is a fixed-size array field of the live
        // `setup_data`, so the slice covers aligned, in-bounds bytes;
        // missing NUL is a `from_bytes_until_nul` error, not UB.
        let block_name = CStr::from_bytes_until_nul(unsafe {
            std::slice::from_raw_parts(setup_data.name.as_ptr() as *const u8, setup_data.name.len())
        })
        .map_err(|_| BlktraceError::InvalidBlockName)?
        .to_str()
        .map_err(|_| BlktraceError::InvalidBlockName)?
        .to_owned();

        // SAFETY: fd holds a configured blktrace from the setup ioctl above.
        unsafe { ioctl::blktrace_start(fd.as_raw_fd()) }.map_err(BlktraceError::Start)?;

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<BlkTraceEvent>();
        let cpu_count = num_cpus::get();
        // The kernel tracer is live from here on; every fallible step below
        // returns through this value's Drop, which cancels the readers,
        // joins them and issues STOP/TEARDOWN.
        let mut session = Self {
            device,
            block_name,
            cancel: Arc::clone(&cancel),
            readers: Vec::with_capacity(cpu_count),
            collector: None,
        };

        for cpu in 0..cpu_count {
            let tx = tx.clone();
            let cancel = Arc::clone(&cancel);
            let relay_channel =
                format!("/sys/kernel/debug/block/{}/trace{cpu}", session.block_name);
            let reader = thread::Builder::new()
                .name(format!("blktrace-reader-{cpu}"))
                .spawn(move || read_relay_channel(&relay_channel, &cancel, tx))?;
            session.readers.push(reader);
        }
        // The collector finishes once every reader has dropped its sender.
        drop(tx);

        let collector = thread::Builder::new()
            .name("blktrace-collector".to_owned())
            .spawn(move || {
                let mut events = Vec::new();
                while let Ok(event) = rx.recv() {
                    events.push(event);
                }
                events
            })?;
        session.collector = Some(collector);

        Ok(session)
    }

    /// Stops tracing and returns everything that was recorded.
    pub fn shutdown(mut self) -> Result<BlkTraceReport, BlktraceError> {
        // Flush the device buffers first: events the kernel is still
        // holding land in the relay channels while we wait.
        // SAFETY: fd is the block device this session was started on.
        unsafe { ioctl::blkflsbuf(self.device.as_raw_fd()) }
            .map_err(BlktraceError::FlushBuffers)?;

        thread::sleep(FLUSH_WAIT);

        self.cancel.store(true, Ordering::Relaxed);

        // shutdown consumes the session, so the collector is always there;
        // the None arm exists only because take() needs an Option.
        let events = match self.collector.take() {
            Some(collector) => collector
                .join()
                .map_err(|_| BlktraceError::ThreadPanicked)?,
            None => Vec::new(),
        };

        // The readers exit on the cancel flag above, which is also what
        // lets the collector's drain end; joining keeps their panics and
        // read errors visible here.
        for reader in self.readers.drain(..) {
            reader.join().map_err(|_| BlktraceError::ThreadPanicked)??;
        }

        let dropped = self.read_dropped()?;
        if dropped > 0 {
            tracing::warn!("Dropped {dropped} blktrace events, the recording is incomplete");
        }

        // Dropping `self` issues BLKTRACESTOP/BLKTRACETEARDOWN.
        drop(self);
        Ok(BlkTraceReport { events, dropped })
    }

    fn read_dropped(&self) -> Result<u64, BlktraceError> {
        let path = format!("/sys/kernel/debug/block/{}/dropped", self.block_name);
        let content = std::fs::read_to_string(path).map_err(BlktraceError::ReadDropped)?;
        content
            .trim()
            .parse::<u64>()
            .map_err(|_| BlktraceError::ParseDropped)
    }
}

impl Drop for BlkTraceSession {
    fn drop(&mut self) {
        // Signal the readers even on the discard path: without this a
        // dropped session would leave its reader threads spinning on the
        // relay channels forever.
        self.cancel.store(true, Ordering::Relaxed);

        // Join the threads before touching the kernel tracer: a reader
        // must not still be reading a relay channel when it is torn down,
        // and this is what makes a failed `start` clean up fully. shutdown
        // has drained both lists by the time it reaches here.
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(collector) = self.collector.take() {
            // Every reader sender is gone after the joins above, so the
            // collector's drain has ended.
            let _ = collector.join();
        }

        // SAFETY: fd is the block device this session was started on.
        if let Err(e) = unsafe { ioctl::blktrace_stop(self.device.as_raw_fd()) } {
            tracing::warn!("Failed to BLKTRACESTOP: {e}");
        }
        // SAFETY: same fd, and the tracer is stopped above.
        if let Err(e) = unsafe { ioctl::blktrace_teardown(self.device.as_raw_fd()) } {
            tracing::warn!("Failed to BLKTRACETEARDOWN: {e}");
        }
    }
}

/// Reads events off one CPU's relay channel until the session is cancelled.
fn read_relay_channel(
    relay_channel: &str,
    cancel: &AtomicBool,
    tx: mpsc::Sender<BlkTraceEvent>,
) -> Result<(), BlktraceError> {
    let mut file = File::open(relay_channel)?;
    tracing::trace!(relay_channel, "Starting to read from trace file");

    loop {
        match read_event(&mut file) {
            Ok(event) => {
                // A send error means the collector is gone; nothing left
                // to do but stop.
                if tx.send(event).is_err() {
                    return Ok(());
                }
            }
            Err(BlktraceError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                if cancel.load(Ordering::Relaxed) {
                    // The session is stopping: give the kernel one last
                    // chance to hand over what it has flushed.
                    thread::sleep(RETRY_INTERVAL);
                    match read_event(&mut file) {
                        Ok(event) => {
                            let _ = tx.send(event);
                        }
                        Err(BlktraceError::Io(e))
                            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                        {
                            tracing::trace!(relay_channel, "Cancelled now");
                            return Ok(());
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    // The channel is empty for now; the kernel appends
                    // records as the traced I/O happens.
                    thread::sleep(RETRY_INTERVAL);
                }
            }
            Err(e) => return Err(e),
        }
    }
}
