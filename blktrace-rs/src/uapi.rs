//! Kernel UAPI: the bindgen bindings for `linux/blktrace_api.h` and the
//! trace ioctls.

mod bindings {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    #![allow(unused)]
    #![allow(missing_docs)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::*;

pub(crate) mod ioctl {
    use super::bindings::blk_user_trace_setup;
    use nix::{ioctl_none, ioctl_readwrite};

    // https://github.com/torvalds/linux/blob/586de92313fcab8ed84ac5f78f4d2aae2db92c59/include/uapi/linux/fs.h#L201-L204
    ioctl_readwrite!(blktrace_setup, 0x12, 115, blk_user_trace_setup);
    ioctl_none!(blktrace_start, 0x12, 116);
    ioctl_none!(blktrace_stop, 0x12, 117);
    ioctl_none!(blktrace_teardown, 0x12, 118);

    // blockdev --flushbufs
    ioctl_none!(blkflsbuf, 0x12, 97);
}
