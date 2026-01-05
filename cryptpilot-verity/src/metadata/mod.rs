// FlatBuffers serialization and deserialization utilities

#![allow(clippy::all)]
#![allow(warnings)]
#[allow(unused_imports, dead_code)]
mod metadata_generated;
#[allow(unused_imports, dead_code)]
mod metadata_hash_generated;

pub use metadata_generated::cryptpilot::verity::{
    FileInfo, FileInfoArgs, FsVerityDescriptor, FsVerityDescriptorArgs, Metadata, MetadataArgs,
};
pub use metadata_hash_generated::cryptpilot::verity::hash::{
    FileHashEntry, FileHashEntryArgs, MetadataHash, MetadataHashArgs,
};

use anyhow::{bail, Result};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use sha2::digest::typenum::Unsigned;
use sha2::{digest::OutputSizeUser, Digest, Sha256};
use verity_core::digest::{FsVeritySha256, InnerHash};
use verity_core::tree::MerkleTree;

/// File information with fs-verity data
pub struct FileVerityInfo {
    pub path: String,
    pub descriptor: verity_core::descriptor::FsVerityDescriptor<sha2::Sha256>,
    pub merkle_tree_level1: Vec<u8>,
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
        // convert merkle_tree_level1 to Vec<digest::Output<D>>
        let merkle_tree_level1: Vec<sha2::digest::Output<Sha256>> = {
            const chunk_size: usize = <Sha256 as OutputSizeUser>::OutputSize::USIZE;
            let iter = self.merkle_tree_level1.chunks_exact(chunk_size);
            if !iter.remainder().is_empty() {
                bail!(
                    "Broken merkle tree for {}: level 1 length is {} not a multiple of the hash size {}",
                    self.path,
                    self.merkle_tree_level1.len(),
                    chunk_size
                );
            }
            iter.map(|chunk| {
                let array: [u8; chunk_size] = chunk.try_into().unwrap(); // this should never fail
                From::from(array)
            })
            .collect()
        };

        let merkle_tree = MerkleTree::<Sha256>::from_level1_hashes(merkle_tree_level1);
        let root_hash = merkle_tree
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

/// Calculate fs-verity hash for file data
pub fn calculate_fsverity_hash(
    data: &[u8],
) -> (
    verity_core::descriptor::FsVerityDescriptor<Sha256>,
    MerkleTree<Sha256>,
) {
    // Create FsVerity digest
    let mut digest = FsVeritySha256::<[u8; 0]>::new();
    digest.update(data);

    // Finalize and get descriptor and merkle tree
    digest.finalize_into_fs_verity_stuffs()
}

/// Serialize file information to FlatBuffers format
pub fn serialize_metadata(file_infos: &[FileVerityInfo]) -> Result<Vec<u8>> {
    let mut builder = FlatBufferBuilder::new();

    // Build FileInfo vector
    let mut file_info_offsets = Vec::with_capacity(file_infos.len());
    for info in file_infos {
        let path_offset = builder.create_string(&info.path);
        let descriptor_hash_offset = builder.create_string(&info.descriptor_hash);

        // Create FsVerityDescriptor
        let root_hash_vec = builder.create_vector(info.descriptor.root_hash.as_slice());
        let salt_vec = builder.create_vector(&info.descriptor.salt);

        let descriptor_offset = FsVerityDescriptor::create(
            &mut builder,
            &FsVerityDescriptorArgs {
                version: info.descriptor.version,
                hash_algorithm: info.descriptor.hash_algorithm,
                log_blocksize: info.descriptor.log_blocksize,
                data_size: info.descriptor.data_size,
                root_hash: Some(root_hash_vec),
                salt: Some(salt_vec),
            },
        );

        // Create merkle tree level 1 vector
        let merkle_tree_vec = builder.create_vector(&info.merkle_tree_level1);

        let file_info = FileInfo::create(
            &mut builder,
            &FileInfoArgs {
                path: Some(path_offset),
                descriptor: Some(descriptor_offset),
                merkle_tree_level1: Some(merkle_tree_vec),
                descriptor_hash: Some(descriptor_hash_offset),
            },
        );
        file_info_offsets.push(file_info);
    }

    let files_vector = builder.create_vector(&file_info_offsets);

    // Create root Metadata table with version
    let metadata = Metadata::create(
        &mut builder,
        &MetadataArgs {
            version: 1, // Current metadata format version
            files: Some(files_vector),
        },
    );

    builder.finish(metadata, None);

    Ok(builder.finished_data().to_vec())
}

/// Deserialize file information from FlatBuffers format
pub fn deserialize_metadata(data: &[u8]) -> Result<Vec<FileVerityInfo>> {
    let metadata = flatbuffers::root::<Metadata>(data)
        .map_err(|e| anyhow::anyhow!("Failed to parse FlatBuffers metadata: {}", e))?;

    // Check metadata version
    let version = metadata.version();
    if version != 1 {
        bail!(
            "Unsupported metadata version: {}. Expected version 1.",
            version
        );
    }

    let mut result = Vec::new();

    if let Some(files) = metadata.files() {
        for file_info in files {
            let path = file_info
                .path()
                .ok_or_else(|| anyhow::anyhow!("Missing path in FileInfo"))?
                .to_string();

            let descriptor_hash = file_info
                .descriptor_hash()
                .ok_or_else(|| anyhow::anyhow!("Missing descriptor_hash in FileInfo"))?
                .to_string();

            let fb_descriptor = file_info
                .descriptor()
                .ok_or_else(|| anyhow::anyhow!("Missing descriptor in FileInfo"))?;

            // Reconstruct descriptor
            let root_hash_bytes = fb_descriptor
                .root_hash()
                .ok_or_else(|| anyhow::anyhow!("Missing root_hash in descriptor"))?;

            let mut root_hash = sha2::digest::generic_array::GenericArray::default();
            root_hash.copy_from_slice(root_hash_bytes.bytes());

            let salt = fb_descriptor
                .salt()
                .map(|s| s.bytes().to_vec())
                .unwrap_or_default();

            let descriptor = verity_core::descriptor::FsVerityDescriptor {
                version: fb_descriptor.version(),
                hash_algorithm: fb_descriptor.hash_algorithm(),
                log_blocksize: fb_descriptor.log_blocksize(),
                data_size: fb_descriptor.data_size(),
                root_hash,
                salt,
            };

            let merkle_tree_level1 = file_info
                .merkle_tree_level1()
                .map(|mt| mt.bytes().to_vec())
                .unwrap_or_default();

            result.push(FileVerityInfo {
                path,
                descriptor,
                merkle_tree_level1,
                descriptor_hash,
            });
        }
    }

    Ok(result)
}

/// Calculate hash from full metadata binary
/// This function:
/// 1. Parses the full metadata
/// 2. Extracts only essential fields (path, descriptor_hash)
/// 3. Serializes them to MetadataHash format
/// 4. Calculates SHA256 hash
pub fn calculate_metadata_hash(metadata_bytes: &[u8]) -> Result<String> {
    // Parse full metadata
    let metadata = flatbuffers::root::<Metadata>(metadata_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse metadata: {}", e))?;

    // Convert full Metadata FlatBuffer to MetadataHash for hash calculation
    let hash_bytes = {
        let mut builder = FlatBufferBuilder::new();

        let files_vector = if let Some(files) = metadata.files() {
            // Build FileHashEntry vector
            let mut files_offsets = Vec::with_capacity(files.len());
            for file_info in files {
                let path = file_info
                    .path()
                    .ok_or_else(|| anyhow::anyhow!("Missing path in FileInfo"))?;

                let descriptor_hash = file_info
                    .descriptor_hash()
                    .ok_or_else(|| anyhow::anyhow!("Missing descriptor_hash in FileInfo"))?;

                let path_offset = builder.create_string(path);
                let hash_offset = builder.create_string(descriptor_hash);

                let entry = FileHashEntry::create(
                    &mut builder,
                    &FileHashEntryArgs {
                        path: Some(path_offset),
                        descriptor_hash: Some(hash_offset),
                    },
                );
                files_offsets.push(entry);
            }

            Some(builder.create_vector(&files_offsets))
        } else {
            None
        };

        // Create MetadataHash table
        let metadata_hash = MetadataHash::create(
            &mut builder,
            &MetadataHashArgs {
                files: files_vector,
            },
        );

        builder.finish(metadata_hash, None);

        // Serialize to MetadataHash format
        builder.finished_data().to_vec()
    };

    // Calculate SHA256
    let mut hasher = sha2::Sha256::new();
    hasher.update(&hash_bytes);

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let test_data = b"test file content";
        let (descriptor, merkle_tree) = calculate_fsverity_hash(test_data);
        let descriptor_hash = hex::encode(descriptor.to_descriptor_hash());
        let info = FileVerityInfo {
            path: "test.txt".to_string(),
            descriptor,
            merkle_tree_level1: merkle_tree.level1_as_bytes(),
            descriptor_hash,
        };

        let file_infos = vec![info];

        let serialized = serialize_metadata(&file_infos).unwrap();
        let deserialized = deserialize_metadata(&serialized).unwrap();

        assert_eq!(file_infos.len(), deserialized.len());
        assert_eq!(file_infos[0].path, deserialized[0].path);
        assert_eq!(
            file_infos[0].descriptor_hash,
            deserialized[0].descriptor_hash
        );
    }
}
