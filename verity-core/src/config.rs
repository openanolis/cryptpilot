use num_enum::TryFromPrimitive;

/// Maximum size of digests, as described [in the Linux kernel documentation](https://www.kernel.org/doc/html/latest/filesystems/fsverity.html#fs-verity-descriptor).
pub const MAX_DIGEST_SIZE: usize = 64;

/// Maximum size of salts, as described [in the Linux kernel documentation](https://www.kernel.org/doc/html/latest/filesystems/fsverity.html#fs-verity-descriptor)
pub const MAX_SALT_SIZE: usize = 32;

/// Linux has a hardcoded limit of 8, see `FS_VERITY_MAX_LEVELS` in `/fs/verity/fsverity_private.h` in the Linux source.
/// In reality you are not likely to hit this limit ever, e.g. with SHA256 you'd need more than `usize::MAX` input bytes.
pub const MAX_LEVELS: usize = 8;

/// Currently the kernel requires the `fs-verity` block size to be equal to the system page size, which is usually 4096.
/// Some modern 64 bit ARM systems [have a 64kB page size](https://www.kernel.org/doc/Documentation/arm64/memory.txt) though.
pub const DEFAULT_BLOCK_SIZE: usize = 4096;

// So we can have easy cross references in doc comments
#[cfg(doc)]
use super::*;

/// Enum of the supported inner hash algorithms.
///
/// The [`Default`] value is `Sha256`, corresponding to the default hash algorithm in the `fsverity` tools, and to the
/// default generic parameter of [`FsVerityDigest`].
///
/// This enum supports conversion to string using [`std::fmt::Display`] and from a string using [`parse_display::FromStr`].
///
/// It also supports conversion to integer using `as u8` and from integer using [`TryFromPrimitive`]). These integers values
/// match the hash algorithm numbering used in the fsverity kernel API.
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    parse_display::FromStr,
    parse_display::Display,
    Debug,
    TryFromPrimitive,
)]
#[display(style = "lowercase")]
#[repr(u8)]
pub enum InnerHashAlgorithm {
    /// As string: `sha256`. As number: `FS_VERITY_HASH_ALG_SHA256` from the kernel API.
    Sha256 = 1,

    /// As string: `sha512`. As number: `FS_VERITY_HASH_ALG_SHA512` from the kernel API.
    Sha512 = 2,
}

impl Default for InnerHashAlgorithm {
    fn default() -> Self {
        Self::Sha256
    }
}
