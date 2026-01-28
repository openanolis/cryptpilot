use digest::Output;

use crate::digest::{salt_to_digest, InnerHash};

/// https://www.kernel.org/doc/html/latest/filesystems/fsverity.html#fs-verity-descriptor
pub struct FsVerityDescriptor<D: InnerHash> {
    pub version: u8,          /* must be 1 */
    pub hash_algorithm: u8,   /* Merkle tree hash algorithm */
    pub log_blocksize: u8,    /* log2 of size of data and tree blocks */
    pub data_size: u64,       /* size of file the Merkle tree is built over */
    pub root_hash: Output<D>, /* Merkle tree root hash */
    pub salt: Vec<u8>,        /* salt prepended to each hashed block, 0 length if none salt */
}

impl<D: InnerHash> FsVerityDescriptor<D> {
    pub fn block_size(&self) -> usize {
        1 << self.log_blocksize
    }

    pub fn to_descriptor_hash(&self) -> digest::Output<D> {
        // the root hash, file size, hash algorithm, and salt are combined into a structure
        // called a 'verity descriptor'. the (salted) hash of this data is the final result,
        // and it is called a 'verity measurement'.
        // https://www.kernel.org/doc/html/latest/filesystems/fsverity.html#fs-verity-descriptor

        let mut digest: D = salt_to_digest(&self.salt);
        digest.update([self.version]);
        digest.update([self.hash_algorithm]);
        digest.update([self.log_blocksize]);
        digest.update([self.salt.len() as u8]);
        digest.update_zeroes(4); // __le32 __reserved_0x04;
        digest.update(self.data_size.to_le_bytes());
        digest.update_padded(&self.root_hash, 64);
        digest.update_padded(&self.salt, 32);
        digest.update_zeroes(144); // __u8 __reserved[144];

        digest.finalize()
    }
}
