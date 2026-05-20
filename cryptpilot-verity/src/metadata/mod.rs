// FlatBuffers serialization and deserialization utilities

#[rustfmt::skip]
#[allow(clippy::all)]
#[allow(warnings)]
#[allow(unused_imports, dead_code)]
mod metadata_generated;

pub use metadata_generated::cryptpilot::verity::{
    FileInfo, FileInfoArgs, FsVerityDescriptor, FsVerityDescriptorArgs, KeyValue, KeyValueArgs,
    Metadata, MetadataArgs,
};

use anyhow::{bail, Result};
use canon_json::CanonJsonSerialize;
use flatbuffers::FlatBufferBuilder;
use sha2::digest::typenum::Unsigned;
use sha2::{digest::OutputSizeUser, Digest, Sha256};
use std::collections::BTreeMap;
use verity_core::digest::FsVeritySha256;
use verity_core::tree::MerkleTree;
use verity_fuse::file_verifier::file_verity_info::FileVerityInfo;

/// Deserialized metadata containing file info and labels
pub struct MetadataInfo {
    pub file_infos: Vec<FileVerityInfo>,
    pub labels: BTreeMap<String, String>,
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
pub fn serialize_metadata(
    file_infos: &[FileVerityInfo],
    labels: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    let mut builder = FlatBufferBuilder::new();

    // Sort by path for stable output (using references to avoid copying)
    let mut sorted_refs: Vec<&FileVerityInfo> = file_infos.iter().collect();
    sorted_refs.sort_unstable_by_key(|info| &info.path);

    // Build FileInfo vector in sorted order
    let mut file_info_offsets = Vec::with_capacity(file_infos.len());
    for info in sorted_refs {
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
        let merkle_tree_vec = builder.create_vector(&info.merkle_tree.level1_as_bytes());

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

    // Build labels vector
    let labels_vector = {
        let mut label_offsets = Vec::with_capacity(labels.len());
        for (key, value) in labels {
            let key_offset = builder.create_string(key);
            let value_offset = builder.create_string(value);
            let label = KeyValue::create(
                &mut builder,
                &KeyValueArgs {
                    key: Some(key_offset),
                    value: Some(value_offset),
                },
            );
            label_offsets.push(label);
        }
        Some(builder.create_vector(&label_offsets))
    };

    // Create root Metadata table with version
    let metadata = Metadata::create(
        &mut builder,
        &MetadataArgs {
            version: 1, // Current metadata format version
            files: Some(files_vector),
            labels: labels_vector,
        },
    );

    builder.finish(metadata, None);

    Ok(builder.finished_data().to_vec())
}

/// Deserialize file information from FlatBuffers format
pub fn deserialize_metadata(data: &[u8]) -> Result<MetadataInfo> {
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

            // convert merkle_tree_level1 to Vec<digest::Output<D>>
            let merkle_tree_level1: Vec<sha2::digest::Output<Sha256>> = {
                const CHUNK_SIZE: usize = <Sha256 as OutputSizeUser>::OutputSize::USIZE;
                let iter = merkle_tree_level1.chunks_exact(CHUNK_SIZE);
                if !iter.remainder().is_empty() {
                    bail!(
                        "Broken merkle tree for {}: level 1 length is {} not a multiple of the hash size {}",
                        path,
                        merkle_tree_level1.len(),
                        CHUNK_SIZE
                    );
                }
                iter.map(|chunk| {
                    let array: [u8; CHUNK_SIZE] = chunk.try_into().unwrap(); // this should never fail
                    From::from(array)
                })
                .collect()
            };

            let merkle_tree = MerkleTree::<Sha256>::from_level1_hashes(merkle_tree_level1);

            result.push(FileVerityInfo {
                path,
                descriptor,
                merkle_tree,
                descriptor_hash,
            });
        }
    }

    // Parse labels
    let mut labels = BTreeMap::new();
    if let Some(labels_vec) = metadata.labels() {
        for kv in labels_vec {
            let key = kv
                .key()
                .ok_or_else(|| anyhow::anyhow!("Missing key in KeyValue"))?
                .to_string();
            let value = kv
                .value()
                .ok_or_else(|| anyhow::anyhow!("Missing value in KeyValue"))?
                .to_string();
            labels.insert(key, value);
        }
    }

    Ok(MetadataInfo {
        file_infos: result,
        labels,
    })
}

/// Calculate hash from full metadata binary
/// This function:
/// 1. Parses the full metadata
/// 2. Extracts only essential fields (path, descriptor_hash)
/// 3. Serializes them to canonical JSON (sorted by path, sorted keys via struct order)
/// 4. Calculates SHA256 hash
pub fn calculate_metadata_hash(metadata_bytes: &[u8]) -> Result<String> {
    let metadata = flatbuffers::root::<Metadata>(metadata_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse metadata: {}", e))?;

    // Extract path + descriptor_hash pairs into a Vec, sorted by path
    let mut entries: Vec<FileHashJsonEntry> = Vec::new();
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
            entries.push(FileHashJsonEntry {
                descriptor_hash,
                path,
            });
        }
    }
    // FlatBuffers files are already sorted by path (see serialize_metadata),
    // but sort again for determinism.
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let doc = MetadataHashDoc { files: entries };
    let json_bytes = doc
        .to_canon_json_vec()
        .map_err(|e| anyhow::anyhow!("marshal canonical JSON: {}", e))?;

    let mut hasher = sha2::Sha256::new();
    hasher.update(&json_bytes);

    Ok(hex::encode(hasher.finalize()))
}

/// Canonical JSON document for metadata hash calculation.
#[derive(serde::Serialize)]
struct MetadataHashDoc {
    files: Vec<FileHashJsonEntry>,
}

/// Single file entry for hash calculation.
#[derive(serde::Serialize)]
struct FileHashJsonEntry {
    descriptor_hash: String,
    path: String,
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
            merkle_tree,
            descriptor_hash,
        };

        info.verify_self().unwrap();

        let file_infos = vec![info];
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());

        let serialized = serialize_metadata(&file_infos, &labels).unwrap();
        let deserialized = deserialize_metadata(&serialized).unwrap();

        assert_eq!(file_infos.len(), deserialized.file_infos.len());
        assert_eq!(file_infos[0].path, deserialized.file_infos[0].path);
        assert_eq!(
            file_infos[0].descriptor_hash,
            deserialized.file_infos[0].descriptor_hash
        );
        assert_eq!(deserialized.labels.get("env"), Some(&"prod".to_string()));
    }

    #[test]
    fn test_serialize_deserialize_empty_labels() {
        let test_data = b"test file content";
        let (descriptor, merkle_tree) = calculate_fsverity_hash(test_data);
        let descriptor_hash = hex::encode(descriptor.to_descriptor_hash());
        let info = FileVerityInfo {
            path: "test.txt".to_string(),
            descriptor,
            merkle_tree,
            descriptor_hash,
        };

        let file_infos = vec![info];
        let labels = BTreeMap::new();

        let serialized = serialize_metadata(&file_infos, &labels).unwrap();
        let deserialized = deserialize_metadata(&serialized).unwrap();

        assert_eq!(file_infos.len(), deserialized.file_infos.len());
        assert!(deserialized.labels.is_empty());
    }

    #[test]
    fn test_canonical_json_hash_determinism() {
        let test_data = b"test file content";
        let (descriptor, merkle_tree) = calculate_fsverity_hash(test_data);
        let descriptor_hash = hex::encode(descriptor.to_descriptor_hash());
        let info = FileVerityInfo {
            path: "test.txt".to_string(),
            descriptor,
            merkle_tree,
            descriptor_hash,
        };

        let file_infos = vec![info];
        let labels = BTreeMap::new();

        let serialized = serialize_metadata(&file_infos, &labels).unwrap();
        let hash1 = calculate_metadata_hash(&serialized).unwrap();

        // Same input → same hash
        let hash2 = calculate_metadata_hash(&serialized).unwrap();
        assert_eq!(hash1, hash2);

        // Different path → different hash
        let (descriptor2, merkle_tree2) = calculate_fsverity_hash(test_data);
        let descriptor_hash2 = hex::encode(descriptor2.to_descriptor_hash());
        let info2 = FileVerityInfo {
            path: "other.txt".to_string(),
            descriptor: descriptor2,
            merkle_tree: merkle_tree2,
            descriptor_hash: descriptor_hash2,
        };
        let serialized2 = serialize_metadata(&[info2], &labels).unwrap();
        let hash3 = calculate_metadata_hash(&serialized2).unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn cross_check_metadata_hash() {
        // FlatBuffers generated by Go: one FileInfo with path="test.txt", descriptor_hash="abcdef1234567890"
        let fb_hex = "0c0000000800080000000400080000000400000001000000100000000c000c0008000000000004000c000000080000001c00000010000000616263646566313233343536373839300000000008000000746573742e74787400000000";
        let fb_bytes = hex::decode(fb_hex).unwrap();
        let hash = calculate_metadata_hash(&fb_bytes).unwrap();
        // Expected: SHA-256 of canonical JSON: {"files":[{"descriptor_hash":"abcdef1234567890","path":"test.txt"}]}
        let expected = "baa8151afa2b0a8eec0175239a0ddcf92cade8d7c364f1a26e5afa0d667335c1";
        assert_eq!(hash, expected, "Rust canonical JSON hash should match Go's");
    }
}
