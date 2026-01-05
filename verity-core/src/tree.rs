use crate::digest::{FsVerityDigest, InnerHash};

use digest::typenum::Unsigned;
use sha2::Digest;

#[derive(Default, Clone)]
pub struct MerkleTree<D: InnerHash>(Vec<digest::Output<D>>);

impl<D: InnerHash> MerkleTree<D> {
    pub fn append_level_1_hash(&mut self, hash: digest::Output<D>) {
        self.0.push(hash);
    }

    /// Get level 1 hashes as concatenated binary data
    pub fn level1_as_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|h| h.as_slice().iter().copied())
            .collect()
    }

    pub fn from_level1_hashes(hashes: Vec<digest::Output<D>>) -> Self {
        Self(hashes)
    }

    pub fn rebuild_root_hash<S>(&self, salt: S, block_size: usize) -> digest::Output<D>
    where
        S: AsRef<[u8]> + Clone + Default,
    {
        // If there's only one hash, it is the root hash.
        if self.0.len() == 1 {
            if let Some(hash) = self.0.iter().next() {
                return hash.clone();
            }
        }

        let mut digest = FsVerityDigest::<D, S>::new_with_salt_and_block_size(salt, block_size);
        for hash in self.0.iter() {
            digest.update(hash);
        }
        let padding =
            (block_size - ((self.0.len() * D::OutputSize::USIZE) % block_size)) % block_size;

        digest.update(vec![0u8; padding]);
        let (descriptor, _) = digest.finalize_into_fs_verity_stuffs();
        descriptor.root_hash
    }

    pub fn verify_data_block(&self, block_index: usize, block_size: usize, data: &[u8]) -> bool {
        if data.len() > block_size {
            return false;
        }

        let Some(expected) = self.0.get(block_index) else {
            return false;
        };

        let mut digest = D::new();
        digest.update_padded(data, block_size);
        let real = digest.finalize();
        return expected == &real;
    }
}
