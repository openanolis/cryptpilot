use std::{
    ffi::CStr,
    mem::MaybeUninit,
    os::fd::{AsFd, AsRawFd},
    path::Path,
};

use anyhow::{bail, ensure, Context, Result};
use nix::{ioctl_none, ioctl_readwrite, mount::MsFlags};
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncReadExt as _,
    select,
};

mod gen {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub use gen::*;
use tokio_util::sync::CancellationToken;

// https://github.com/torvalds/linux/blob/586de92313fcab8ed84ac5f78f4d2aae2db92c59/include/uapi/linux/fs.h#L201-L204
ioctl_readwrite!(blktrace_setup, 0x12, 115, blk_user_trace_setup);
ioctl_none!(blktrace_start, 0x12, 116);
ioctl_none!(blktrace_stop, 0x12, 117);
ioctl_none!(blktrace_teardown, 0x12, 118);

pub struct BlkTrace {
    task: BlkTraceTask,
    join_handle: tokio::task::JoinHandle<Result<Vec<BlkTraceEvent>>>,
    cancel_token: CancellationToken,
}

pub struct BlkTraceTask {
    pub file: File,
}

#[derive(Debug)]
pub struct BlkTraceEvent {
    pub event: blk_io_trace,
    pub data: Vec<u8>,
}

// The size of each buffer for blktrace
const BLK_TRACE_BUF_SIZE: u32 = 65536;
// The amount of buffers for blktrace to keep spare
const BLK_TRACE_BUF_COUNT: u32 = 16;

impl BlkTrace {
    async fn check_and_setup_debugfs() -> Result<()> {
        let debugfs = Path::new("/sys/kernel/debug");
        if !debugfs.exists() {
            bail!("The debugfs (/sys/kernel/debug) is not supported in current kernel, please enable it");
        }

        let mounted = mnt::MountIter::new_from_proc()?.any(|item| {
            if let Ok(item) = item {
                return item.file == debugfs;
            }
            false
        });
        if !mounted {
            tracing::info!("debugfs not mounted, mounting it now");
            tokio::fs::create_dir_all(&debugfs).await?;

            tokio::task::spawn_blocking(move || -> Result<_> {
                nix::mount::mount(
                    Some("debugfs"),
                    debugfs,
                    Some("debugfs"),
                    MsFlags::empty(),
                    Option::<&str>::None,
                )
                .context("Failed to mount debugfs")?;
                Ok(())
            })
            .await??;
        }

        Ok(())
    }

    pub async fn monitor(path: impl AsRef<Path>) -> Result<Self> {
        Self::check_and_setup_debugfs().await?;

        let file = File::open(path).await?;
        let task = BlkTraceTask { file };

        let fd = task.file.as_fd();

        let mut setup_data = blk_user_trace_setup {
            // Capture read and write events only
            // See: https://github.com/sdsc/blktrace/blob/dd093eb1c48e0d86b835758b96a9886fb7773aa4/act_mask.c#L40
            act_mask: (blktrace_cat_BLK_TC_READ | blktrace_cat_BLK_TC_WRITE) as u16,
            buf_size: BLK_TRACE_BUF_SIZE,
            buf_nr: BLK_TRACE_BUF_COUNT,
            ..Default::default()
        };
        let _ = unsafe { blktrace_setup(fd.as_raw_fd(), &mut setup_data) }
            .context("Failed to BLKTRACESETUP")?;

        let block_name = CStr::from_bytes_until_nul(unsafe {
            &*(setup_data.name.as_slice() as *const [i8] as *const [u8])
        })
        .context("Block name not vaild")?
        .to_str()
        .context("Block name not vaild utf-8 string")?; // use the kernel returned block device name

        let _ = unsafe { blktrace_start(fd.as_raw_fd()) }.context("Failed to BLKTRACESTART")?;

        // Open the relay channel for each cpu
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        let cancel_token = CancellationToken::new();

        let num_cpus = num_cpus::get();

        let join_handles =
            (0..num_cpus)
                .map(|i| {
                    let tx = tx.clone();
                    let cancel_token = cancel_token.clone();
                    let relay_channel =
                        format!("/sys/kernel/debug/block/{}/trace{}", block_name, i);

                    tokio::spawn(async move {
                        // Open the relay channel
                        let mut file = OpenOptions::new()
                            .read(true)
                            .open(&relay_channel)
                            .await
                            .context("Failed to open trace file")?;

                        tracing::trace!(relay_channel, "Starting to read from trace file");

                        loop {
                            // Read blk_event from relay channel
                            let mut blk_event = MaybeUninit::<blk_io_trace>::uninit();
                            let ptr = unsafe {
                                std::mem::transmute::<
                                    _,
                                    &mut [u8; std::mem::size_of::<blk_io_trace>()],
                                >(blk_event.as_mut_ptr())
                            };

                            let res = select! {
                                res = file.read_exact(ptr) => {
                                    res
                                }
                                _ = cancel_token.cancelled() => {
                                    tracing::trace!(relay_channel, "Cancelled now");
                                    break;
                                },
                            };

                            if let Err(e) = &res {
                                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                                    // Try agein
                                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                    continue;
                                }
                            }
                            res.context("Failed to read trace event from trace file")?;
                            let blk_event = unsafe { blk_event.assume_init() };

                            ensure!(
                                blk_event.magic == BLK_IO_TRACE_MAGIC + BLK_IO_TRACE_VERSION,
                                "blktrace event magic number mismatch"
                            );

                            let mut data = vec![0; blk_event.pdu_len as usize];
                            file.read_exact(&mut data)
                                .await
                                .context("Failed to read trace event data from trace file")?;
                            tracing::trace!(relay_channel, "Got a new blk_event");

                            tx.send(BlkTraceEvent {
                                event: blk_event,
                                data,
                            })
                            .await
                            .context("Failed to send trace event to channel")?;
                        }

                        anyhow::Result::<_>::Ok(())
                    })
                })
                .collect::<Vec<_>>();

        let join_handle = tokio::spawn(async move {
            let mut traces = vec![];
            while let Some(blk_event) = rx.recv().await {
                traces.push(blk_event);
            }

            for task in join_handles {
                task.await??;
            }

            Ok(traces)
        });

        Ok(Self {
            task,
            join_handle,
            cancel_token,
        })
    }

    pub async fn shutdown(self) -> Result<Vec<BlkTraceEvent>> {
        // Wait a millisecond to make sure all the trace is generated and put on the relay channel by kernel
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        self.cancel_token.cancel();
        let res = self.join_handle.await??;
        drop(self.task);
        Ok(res)
    }
}

impl Drop for BlkTraceTask {
    fn drop(&mut self) {
        if let Err(e) = unsafe { blktrace_stop(self.file.as_raw_fd()) } {
            tracing::warn!("Failed to BLKTRACESTOP: {e}")
        };
        if let Err(e) = unsafe { blktrace_teardown(self.file.as_raw_fd()) } {
            tracing::warn!("Failed to BLKTRACETEARDOWN: {e}")
        };
    }
}

#[cfg(test)]
pub mod tests {

    use std::io::SeekFrom;

    use crate::fs::block::devicemapper::DeviceMapperDevice;

    use super::*;
    use anyhow::Result;
    use tokio::io::{AsyncSeekExt, BufReader};

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_blktrace() -> Result<()> {
        let dm_device = DeviceMapperDevice::new_zero(10 * 1024 * 1024 * 1024).await?;
        let device_path = dm_device.path();

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        tracing::info!("The tracer is now enabled");
        let tracer = BlkTrace::monitor(&device_path).await?;

        tracing::info!("Start to randomly read the block");
        {
            let f = File::open(device_path).await?;
            let mut reader = BufReader::new(f);
            reader.seek(SeekFrom::Start(0)).await?;
            reader.read_exact(&mut [0; 125]).await?;

            reader.seek(SeekFrom::Start(4096)).await?;
            reader.read_exact(&mut [0; 1025]).await?;
        }

        tracing::info!("Finished to randomly read the block");

        let traces = tracer.shutdown().await?;

        let count_read = traces
            .iter()
            .filter(|t: &&BlkTraceEvent| {
                (t.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_READ
                    == blktrace_cat_BLK_TC_READ
            })
            .count();

        let count_write = traces
            .iter()
            .filter(|t| {
                (t.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_WRITE
                    == blktrace_cat_BLK_TC_WRITE
            })
            .count();

        tracing::info!(
            "The tracer is now shutdown, got {} traces, {count_read} reads, {count_write} writes",
            traces.len()
        );
        for trace in &traces {
            tracing::info!("{trace:?}");
        }

        assert!(traces.len() > 0);
        assert!(count_read > 0);

        Ok(())
    }
}
