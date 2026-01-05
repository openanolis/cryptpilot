// FlatBuffers serialization and deserialization utilities

#![allow(clippy::all)]
#![allow(warnings)]
#[allow(unused_imports, dead_code)]
mod metadata_generated;

pub use metadata_generated::cryptpilot::verity::{
    FileInfo, FileInfoArgs, FsVerityDescriptor, FsVerityDescriptorArgs, Metadata, MetadataArgs,
};

use anyhow::Result;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use sha2::Digest;
use verity_core::digest::{FsVeritySha256, InnerHash};

/// File information with fs-verity data
pub struct FileVerityInfo {
    pub path: String,
    pub descriptor: verity_core::descriptor::FsVerityDescriptor<sha2::Sha256>,
    pub merkle_tree_level1: Vec<u8>,
    pub descriptor_hash: String,
}

/// Calculate fs-verity hash for file data
pub fn calculate_fsverity_hash(data: &[u8]) -> Result<FileVerityInfo> {
    use sha2::digest::Digest as _;
    
    // Create FsVerity digest
    let mut digest = FsVeritySha256::<[u8; 0]>::new();
    digest.update(data);
    
    // Finalize and get descriptor and merkle tree
    let (descriptor, merkle_tree) = digest.finalize_into_fs_verity_stuffs();
    
    // Get merkle tree level 1 hashes as binary data
    let merkle_tree_level1: Vec<u8> = merkle_tree.0.iter().flat_map(|h| h.as_slice().iter().copied()).collect();
    
    // Calculate descriptor hash (final measurement)
    let descriptor_hash = hex::encode(descriptor.to_descriptor_hash());
    
    Ok(FileVerityInfo {
        path: String::new(), // Will be set by caller
        descriptor,
        merkle_tree_level1,
        descriptor_hash,
    })
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
    
    // Create root Metadata table
    let metadata = Metadata::create(
        &mut builder,
        &MetadataArgs {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let test_data = b"test file content";
        let mut info = calculate_fsverity_hash(test_data).unwrap();
        info.path = "test.txt".to_string();
        
        let file_infos = vec![info];
        
        let serialized = serialize_metadata(&file_infos).unwrap();
        let deserialized = deserialize_metadata(&serialized).unwrap();
        
        assert_eq!(file_infos.len(), deserialized.len());
        assert_eq!(file_infos[0].path, deserialized[0].path);
        assert_eq!(file_infos[0].descriptor_hash, deserialized[0].descriptor_hash);
    }
}
