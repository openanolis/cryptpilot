# blktrace-rs

Linux block-layer I/O trace capture, straight from the kernel.

`blktrace_rs` drives the kernel block tracing interface directly: the
`BLKTRACESETUP2`/`BLKTRACESETUP`/`BLKTRACESTART`/`BLKTRACESTOP`/
`BLKTRACETEARDOWN` ioctls plus the per-CPU relay channels under debugfs.
It needs neither the `blktrace` nor the `blkparse` command line tools,
and it gives you parsed, normalized trace records as Rust structs.

## Usage

```rust
use blktrace_rs::BlkTrace;

let session = BlkTrace::monitor("/dev/sda")?;
// Reads served from the page cache never reach the block layer:
BlkTrace::drop_page_cache()?;
// ... do the I/O you want traced ...
let report = session.shutdown()?;
for event in &report.events {
    if event.is_read() {
        println!("read {} bytes at sector {}", event.bytes, event.sector);
    }
}
assert_eq!(report.dropped, 0);
```

With `features = ["async"]` the crate adds `monitor_async` and
`shutdown_async`, which run the blocking core on the tokio blocking pool
so async callers never stall their executor.

## Trace formats

Sessions try the kernel's v2 trace API first (`BLKTRACESETUP2`, available
since Linux 6.19). It is the only format that records write-zeroes
requests: since kernel commit bc49af56eea8 they carry their own trace
category, above the v1 category ceiling, and v1 sessions silently drop
such records. On older kernels the setup falls back to the v1 API, where
write-zeroes requests are recorded as ordinary writes. Either way the
records are normalized into `BlkTraceEvent`/`BlkIoKind`, so the version
in use never changes what callers see (`is_write_zeroes()` reports the
requests on 6.19+, while on older kernels they arrive via `is_write()`).

## Requirements

- Linux with debugfs, and root to run (the ioctls and debugfs require it).
- Nothing to install to build: every kernel UAPI layout is hand-written
  and pinned to upstream header commits, so the crate needs neither
  `libclang` nor kernel UAPI headers, and builds on any kernel version.

## License

Apache-2.0, see [LICENSE](LICENSE).
