use anyhow::{bail, Result};
use sha2::Sha256;
use verity_core::descriptor::FsVerityDescriptor;
use verity_core::tree::MerkleTree;

/// File information with fs-verity data
pub struct FileVerityInfo {
    pub path: String,
    pub descriptor: FsVerityDescriptor<Sha256>,
    pub merkle_tree: MerkleTree<Sha256>,
    pub descriptor_hash: String,
}

impl FileVerityInfo {
    pub fn verify_self(&self) -> Result<()> {
        let calculated_descriptor_hash = hex::encode(self.descriptor.to_descriptor_hash());
        if calculated_descriptor_hash != self.descriptor_hash {
            bail!(
                "Descriptor hash mismatch for {}, expected {}, got {}",
                self.path,
                self.descriptor_hash,
                calculated_descriptor_hash
            );
        }
        let root_hash = self
            .merkle_tree
            .rebuild_root_hash(self.descriptor.salt.clone(), self.descriptor.block_size());
        if root_hash != self.descriptor.root_hash {
            bail!(
                "Merkle tree root hash mismatch for {}, expected {}, got {}",
                self.path,
                hex::encode(self.descriptor.root_hash),
                hex::encode(root_hash)
            );
        }

        Ok(())
    }
}

impl std::fmt::Debug for FileVerityInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileVerityInfo")
            .field("path", &self.path)
            .field("descriptor_hash", &self.descriptor_hash)
            .finish()
    }
}
