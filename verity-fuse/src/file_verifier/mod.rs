use relative_path::RelativePath;
use std::fmt::Debug;

pub mod file_verity_info;
pub mod verity_verifier;

/// Trait for individual file entry with verification capability
///
/// Each file tracked by the verifier implements this trait to provide:
/// - Inode and path information
/// - Expected file size from fs-verity descriptor
/// - Block size for verification
/// - Block-level data verification
pub trait VerifiableFile: Debug + Send + Sync {
    /// Get the inode number of this file
    fn ino(&self) -> u64;

    /// Get the path of this file
    fn path(&self) -> &RelativePath;

    /// Get the expected file size from fs-verity descriptor
    /// Returns None for directories
    fn expected_size(&self) -> Option<u64>;

    /// Get the block size from fs-verity descriptor
    /// Returns None for directories
    fn block_size(&self) -> Option<u32>;

    /// Verify a data block for this file
    ///
    /// # Arguments
    /// * `block_index` - Index of the block to verify (0-based)
    /// * `data` - The actual data block to verify
    ///
    /// # Returns
    /// * `Ok(())` if verification succeeds
    /// * `Err(_)` if verification fails
    fn verify_data_block(&self, block_index: usize, data: &[u8]) -> anyhow::Result<()>;
}

/// Trait for file verification and inode mapping
///
/// Implementations of this trait provide:
/// - Lookup files by inode or path
/// - Access to per-file verification data via associated VerifiableFile type
pub trait FileVerifier: Debug + Send + Sync {
    /// The type of verifiable file this verifier produces
    type File: VerifiableFile;

    /// Look up a verifiable file by inode number
    ///
    /// # Returns
    /// * `Some(&File)` - Reference to the verifiable file if found
    /// * `None` - If the inode is not tracked or is a directory/symlink without verification data
    fn lookup_by_ino(&self, ino: u64) -> Option<&Self::File>;

    /// Look up a verifiable file by path
    ///
    /// # Returns
    /// * `Some(&File)` - Reference to the verifiable file if found
    /// * `None` - If the path is not tracked or is a directory/symlink without verification data
    fn lookup_by_path(&self, path: &RelativePath) -> Option<&Self::File>;
}
