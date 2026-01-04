//! This crate implements utilities for working with `fs-verity` functionality of the Linux kernel.
//!
//! ## What is `fs-verity`?
//!
//! This allows you to make a file permanently immutable and have the kernel calculate and store
//! a Merkle tree hash for the file that will be instantly retrievable from that point on. The
//! kernel will also refuse to load any part of the file into memory unless it matches the
//! corresponding parts of the Merkle tree, meaning as long as you trust the kernel the fs-verity
//! measurement returned for a file will always match the data you read from that file. This means
//! that even if the filesystem was tampered with while the system was off, the file contents
//! cannot be changed without also changing or removing the verity hash, or causing reads from the
//! file to fail.
//!
//! TLDR: It allows you to make a file immutable and allows you to instantly get a reliable hash
//! for that file at any time.
//!
//! ## What is this crate?
//!
//! You can think of it as a pure Rust replacement for the main parts of `libfsverity`.
//!
//! It consists of two parts:
//!
//! * An implementation of [`sha2::digest::Digest`] which can calculate an `fs-verity` measurement
//!   in userland Rust code. This is useful for e.g. servers and build systems which may not want
//!   to (or be able to) enable `fs-verity` for files, but still need to know the digest values
//!   so they can be used to create e.g. a signed manifest file containing all the digests.
//! * On Linux systems, two functions which allow you to enable `fs-verity` for a file, and to
//!   fetch the digest for the file from the kernel once it's enabled. This uses the `ioctl`
//!   interface directly.
//!
//! This was mostly produced as a coding exercise. It works and is pretty clean (well, as much
//! as is possible under the API constraints I set myself) and thoroughly commented, but it is
//! currently a bit slower than the C implementation because that uses `libcrypto`'s assembly
//! code for SHA256, and the SHA256 implementation in RustCrypto is not quite as fast.
//!
//! One neat aspect of this implementation is that it works in a completely streaming manner,
//! accepting any amount of input in chunks of whatever size.

pub mod config;
pub mod descriptor;
pub mod digest;
pub mod tree;
