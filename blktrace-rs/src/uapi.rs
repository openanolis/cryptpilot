//! Kernel UAPI: blktrace record and setup layouts, trace category bits,
//! and the block ioctls, all hand-written against pinned upstream headers.
//!
//! Every layout below mirrors a C type from a kernel UAPI header at a
//! fixed commit; the doc comments give the pinned URL and the
//! compile-time assertions re-check the mirror. Hand-writing them keeps
//! the crate free of bindgen, libclang and kernel headers at build time,
//! and lets it build against a header set too old to know the v2 types:
//! those arrived only with kernel 6.19 (setup ioctl 915bb53860c3, record
//! 4ae8efb4f907, write-zeroes category bc49af56eea8).

use core::ffi::c_char;

/// `BLK_TC_SHIFT`: a record's trace categories live in its `action`
/// field at and above bit 16; the low 16 bits select the concrete action
/// (queue, issue, complete, ...).
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L43
pub(crate) const BLK_TC_SHIFT: u32 = 16;

/// Trace category bits, from `enum blktrace_cat`,
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L10-L41
/// Only the categories the crate classifies are listed. The
/// `BLK_TC_WRITE_ZEROES` bit (1 << 22) is above the v1 ceiling
/// `BLK_TC_END_V1` (1 << 15), which is why v1 sessions drop such records.
pub(crate) const BLK_TC_READ: u64 = 1 << 0;
pub(crate) const BLK_TC_WRITE: u64 = 1 << 1;
pub(crate) const BLK_TC_FLUSH: u64 = 1 << 2;
pub(crate) const BLK_TC_DISCARD: u64 = 1 << 13;
pub(crate) const BLK_TC_WRITE_ZEROES: u64 = 1 << 22;

/// `BLK_IO_TRACE_MAGIC`: the fixed high bits of a trace record's magic.
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L114
pub(crate) const BLK_IO_TRACE_MAGIC: u32 = 0x65617400;
/// `BLK_IO_TRACE_VERSION`: the magic version byte of v1 records.
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L115
pub(crate) const BLK_IO_TRACE_VERSION: u32 = 0x07;
/// `BLK_IO_TRACE2_VERSION`: the magic version byte of v2 records.
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L116
pub(crate) const BLK_IO_TRACE2_VERSION: u32 = 0x08;

/// `BLKTRACE_BDEV_SIZE`: the v1 setup struct's `name` length.
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L166
const BLKTRACE_BDEV_SIZE: usize = 32;
/// `BLKTRACE_BDEV_SIZE2`: the v2 setup struct's `name` length.
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L167
const BLKTRACE_BDEV_SIZE2: usize = 64;

/// `struct blk_io_trace`, the v1 trace record.
///
/// Mirrors the C struct of the same name,
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L121-L134
/// Its `action` is 32 bits wide, so it cannot carry a category above
/// `BLK_TC_END_V1`; `pdu_len` bytes of payload follow the 48-byte header.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BlkIoTrace {
    pub(crate) magic: u32,
    pub(crate) sequence: u32,
    pub(crate) time: u64,
    pub(crate) sector: u64,
    pub(crate) bytes: u32,
    pub(crate) action: u32,
    pub(crate) pid: u32,
    pub(crate) device: u32,
    pub(crate) cpu: u32,
    pub(crate) error: u16,
    pub(crate) pdu_len: u16,
}

impl Default for BlkIoTrace {
    fn default() -> Self {
        // SAFETY: the struct is plain old data (integer fields only), so
        // an all-zero value is valid.
        unsafe { std::mem::zeroed() }
    }
}

/// `struct blk_io_trace2`, the v2 trace record.
///
/// Mirrors the C struct of the same name,
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L136-L150
/// Like the v1 record, but `action` is 64 bits wide - so categories above
/// the v1 ceiling `BLK_TC_END_V1` survive - and `pad` follows `pdu_len`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BlkIoTrace2 {
    pub(crate) magic: u32,
    pub(crate) sequence: u32,
    pub(crate) time: u64,
    pub(crate) sector: u64,
    pub(crate) bytes: u32,
    pub(crate) pid: u32,
    pub(crate) action: u64,
    pub(crate) device: u32,
    pub(crate) cpu: u32,
    pub(crate) error: u16,
    pub(crate) pdu_len: u16,
    pub(crate) pad: [u8; 12],
}

impl Default for BlkIoTrace2 {
    fn default() -> Self {
        // SAFETY: plain old data, as for `BlkIoTrace`.
        unsafe { std::mem::zeroed() }
    }
}

/// `struct blk_user_trace_setup`, the `BLKTRACESETUP` setup struct.
///
/// Mirrors the C struct of the same name,
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L172-L180
/// C natural alignment pads before `buf_size` and `start_lba`, making the
/// struct 72 bytes; `repr(C)` reproduces that layout exactly.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BlkUserTraceSetup {
    pub(crate) name: [c_char; BLKTRACE_BDEV_SIZE],
    pub(crate) act_mask: u16,
    pub(crate) buf_size: u32,
    pub(crate) buf_nr: u32,
    pub(crate) start_lba: u64,
    pub(crate) end_lba: u64,
    pub(crate) pid: u32,
}

impl Default for BlkUserTraceSetup {
    fn default() -> Self {
        // SAFETY: plain old data, as for `BlkIoTrace`; zeroing also
        // fills the two alignment gaps deterministically.
        unsafe { std::mem::zeroed() }
    }
}

/// `struct blk_user_trace_setup2`, the `BLKTRACESETUP2` setup struct.
///
/// Mirrors the C struct of the same name,
/// https://github.com/torvalds/linux/blob/bc49af56eea866c34d21bf582f65b02fc8c06ec3/include/uapi/linux/blktrace_api.h#L185-L195
/// Compared to v1: `name` is 64 bytes instead of 32 and `act_mask` is 64
/// bits instead of 16; `flags` and `reserved` are unused padding.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct BlkUserTraceSetup2 {
    pub(crate) name: [c_char; BLKTRACE_BDEV_SIZE2],
    pub(crate) act_mask: u64,
    pub(crate) buf_size: u32,
    pub(crate) buf_nr: u32,
    pub(crate) start_lba: u64,
    pub(crate) end_lba: u64,
    pub(crate) pid: u32,
    pub(crate) flags: u32,
    pub(crate) reserved: [u64; 11],
}

impl Default for BlkUserTraceSetup2 {
    fn default() -> Self {
        // SAFETY: plain old data, as for `BlkIoTrace`.
        unsafe { std::mem::zeroed() }
    }
}

// The wire formats above are hand-written, so pin their layout to the
// kernel's at compile time: sizes and field offsets as of the pinned
// commits in the doc comments above.
const _: () = {
    assert!(std::mem::size_of::<BlkIoTrace>() == 48);
    assert!(std::mem::offset_of!(BlkIoTrace, action) == 28);
    assert!(std::mem::offset_of!(BlkIoTrace, pdu_len) == 46);
    assert!(std::mem::size_of::<BlkIoTrace2>() == 64);
    assert!(std::mem::offset_of!(BlkIoTrace2, action) == 32);
    assert!(std::mem::offset_of!(BlkIoTrace2, pdu_len) == 50);
    assert!(std::mem::size_of::<BlkUserTraceSetup>() == 72);
    assert!(std::mem::offset_of!(BlkUserTraceSetup, act_mask) == 32);
    assert!(std::mem::offset_of!(BlkUserTraceSetup, buf_size) == 36);
    assert!(std::mem::offset_of!(BlkUserTraceSetup, start_lba) == 48);
    assert!(std::mem::offset_of!(BlkUserTraceSetup, end_lba) == 56);
    assert!(std::mem::offset_of!(BlkUserTraceSetup, pid) == 64);
    assert!(std::mem::size_of::<BlkUserTraceSetup2>() == 192);
    assert!(std::mem::offset_of!(BlkUserTraceSetup2, act_mask) == 64);
    assert!(std::mem::offset_of!(BlkUserTraceSetup2, flags) == 100);
};

pub(crate) mod ioctl {
    use nix::{ioctl_none, ioctl_readwrite};

    use super::{BlkUserTraceSetup, BlkUserTraceSetup2};

    // Ioctls from include/uapi/linux/fs.h, pinned to commit
    // 21a62663d473475db9f05c108e84f035b33e8484:
    // BLKTRACESETUP   _IOWR(0x12, 115, struct blk_user_trace_setup) L295
    // BLKTRACESTART   _IO(0x12, 116) L296
    // BLKTRACESTOP    _IO(0x12, 117) L297
    // BLKTRACETEARDOWN _IO(0x12, 118) L298
    // https://github.com/torvalds/linux/blob/21a62663d473475db9f05c108e84f035b33e8484/include/uapi/linux/fs.h#L295-L298
    ioctl_readwrite!(blktrace_setup, 0x12, 115, BlkUserTraceSetup);
    ioctl_none!(blktrace_start, 0x12, 116);
    ioctl_none!(blktrace_stop, 0x12, 117);
    ioctl_none!(blktrace_teardown, 0x12, 118);
    // BLKTRACESETUP2 _IOWR(0x12, 142, struct blk_user_trace_setup2) L311
    // https://github.com/torvalds/linux/blob/21a62663d473475db9f05c108e84f035b33e8484/include/uapi/linux/fs.h#L311
    ioctl_readwrite!(blktrace_setup2, 0x12, 142, BlkUserTraceSetup2);
    // BLKFLSBUF _IO(0x12, 97) L273: blockdev --flushbufs
    // https://github.com/torvalds/linux/blob/21a62663d473475db9f05c108e84f035b33e8484/include/uapi/linux/fs.h#L273
    ioctl_none!(blkflsbuf, 0x12, 97);
}
