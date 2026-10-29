# blktrace-rs

Linux block-layer I/O trace capture, straight from the kernel.

`blktrace_rs` drives the kernel block tracing interface directly: the
`BLKTRACESETUP`/`BLKTRACESTART`/`BLKTRACESTOP`/`BLKTRACETEARDOWN` ioctls
plus the per-CPU relay channels under debugfs. It needs neither the
`blktrace` nor the `blkparse` command line tools, and it gives you the
parsed `blk_io_trace` records as Rust structs.

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
        println!("read {} bytes at sector {}", event.event.bytes, event.event.sector);
    }
}
assert_eq!(report.dropped, 0);
```

With `features = ["async"]` the crate adds `monitor_async` and
`shutdown_async`, which run the blocking core on the tokio blocking pool so
async callers never stall their executor.

## Requirements

- Linux with debugfs, and root to run (the ioctls and debugfs require it).
- Building the crate requires `libclang` and the kernel UAPI headers
  (`linux/blktrace_api.h`), via bindgen.

## License

Apache-2.0, see [LICENSE](LICENSE).
