use crate::file_verifier::{FileVerifier, VerifiableFile};
use anyhow::Result;
use relative_path::{RelativePath, RelativePathBuf};
use std::collections::HashMap;

use super::file_verity_info::FileVerityInfo;

pub const ROOT_INO: u64 = 1;
pub const ROOT_PATH: &str = "";

/// Entry kind inside verifier - verity-enabled file or directory
#[derive(Debug)]
pub enum EntryKind {
    VerityEnabled(FileVerityInfo),
    Directory(RelativePathBuf),
}

/// Single filesystem entry tracked by the verifier
#[derive(Debug)]
pub struct FsEntry {
    ino: u64,
    kind: EntryKind,
}

impl VerifiableFile for FsEntry {
    fn ino(&self) -> u64 {
        self.ino
    }

    fn path(&self) -> &RelativePath {
        match &self.kind {
            EntryKind::VerityEnabled(info) => RelativePath::new(&info.path),
            EntryKind::Directory(path) => path.as_relative_path(),
        }
    }

    fn expected_size(&self) -> Option<u64> {
        match &self.kind {
            EntryKind::VerityEnabled(info) => Some(info.descriptor.data_size),
            EntryKind::Directory(_) => None,
        }
    }

    fn block_size(&self) -> Option<u32> {
        match &self.kind {
            EntryKind::VerityEnabled(info) => Some(info.descriptor.block_size() as u32),
            EntryKind::Directory(_) => None,
        }
    }

    fn verify_data_block(&self, block_index: usize, data: &[u8]) -> Result<()> {
        match &self.kind {
            EntryKind::VerityEnabled(info) => {
                let block_size = info.descriptor.block_size();
                let is_valid = info
                    .merkle_tree
                    .verify_data_block(block_index, block_size, data);

                if !is_valid {
                    anyhow::bail!(
                        "Block verification failed for {} (ino {}) block {}",
                        info.path,
                        self.ino,
                        block_index
                    );
                }
                Ok(())
            }
            EntryKind::Directory(path) => {
                anyhow::bail!(
                    "Cannot verify data block for directory {} (ino {})",
                    path,
                    self.ino
                )
            }
        }
    }
}

/// Real verifier that performs actual fs-verity verification
pub struct VerityVerifier {
    path_to_ino: HashMap<RelativePathBuf, u64>,
    ino_to_file: HashMap<u64, FsEntry>,
    next_ino: u64,
}

impl std::fmt::Debug for VerityVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerityVerifier")
            .field("num_paths", &self.path_to_ino.len())
            .field("num_verified_files", &self.ino_to_file.len())
            .finish()
    }
}

impl VerityVerifier {
    /// Helper function to register a new entry and log the mapping
    fn register_entry(&mut self, path: RelativePathBuf, kind: EntryKind) {
        let ino = self.next_ino;
        self.next_ino += 1;

        let kind_str = match &kind {
            EntryKind::Directory(_) => "DIR",
            EntryKind::VerityEnabled(_) => "FILE",
        };

        tracing::info!("Register entry: ino={} -> {} [{}]", ino, path, kind_str);

        self.path_to_ino.insert(path.clone(), ino);
        self.ino_to_file.insert(ino, FsEntry { ino, kind });
    }

    /// Create a new verifier from metadata file information
    ///
    /// # Arguments
    /// * `file_infos` - List of file information from metadata
    pub fn new(file_infos: Vec<FileVerityInfo>) -> Result<Self> {
        let mut verifier = Self {
            path_to_ino: HashMap::new(),
            ino_to_file: HashMap::new(),
            next_ino: ROOT_INO,
        };

        // Insert root directory (uses ROOT_INO and advances next_ino to ROOT_INO + 1)
        let root_path = RelativePathBuf::from(ROOT_PATH);
        verifier.register_entry(
            root_path,
            EntryKind::Directory(RelativePathBuf::from(ROOT_PATH)),
        );

        // Process all files from metadata (verity-enabled files)
        for info in file_infos {
            let path = RelativePathBuf::from(&info.path);

            // Register all parent directories first
            let mut current = path.as_relative_path();
            while let Some(parent) = current.parent() {
                if parent.as_str().is_empty() {
                    break; // Reached root
                }
                let parent_buf = parent.to_relative_path_buf();
                if !verifier.path_to_ino.contains_key(&parent_buf) {
                    verifier.register_entry(parent_buf.clone(), EntryKind::Directory(parent_buf));
                }
                current = parent;
            }

            // Now register the file itself
            verifier.register_entry(path, EntryKind::VerityEnabled(info));
        }

        Ok(verifier)
    }
}

impl FileVerifier for VerityVerifier {
    type File = FsEntry;

    fn lookup_by_ino(&self, ino: u64) -> Option<&FsEntry> {
        self.ino_to_file.get(&ino)
    }

    fn lookup_by_path(&self, path: &RelativePath) -> Option<&FsEntry> {
        let ino = self.path_to_ino.get(path)?;
        self.ino_to_file.get(ino)
    }
}
