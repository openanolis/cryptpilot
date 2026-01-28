use crate::file_verifier::{FileVerifier, VerifiableFile};
use cap_std::fs::{Dir, Metadata};
use cap_std::fs::{MetadataExt as _, PermissionsExt};
use fuser::{FileAttr, FileType, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc;
use relative_path::RelativePath;
use std::ffi::OsStr;
use std::io::Read;
use std::io::{Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const TTL: Duration = Duration::from_secs(1);
const FS_BLOCK_SIZE: u32 = 4096;

pub struct VerityFS<V: FileVerifier> {
    source: Dir,
    verifier: Arc<V>,
}

impl<V: FileVerifier> VerityFS<V> {
    pub fn new(source: &Path, verifier: V) -> anyhow::Result<Self> {
        let dir: Dir = Dir::open_ambient_dir(source, cap_std::ambient_authority())?;
        let verifier = Arc::new(verifier);

        Ok(Self {
            source: dir,
            verifier,
        })
    }

    fn get_metadata(&self, file: &V::File) -> Result<Metadata, i32> {
        self.source
            .symlink_metadata(relative_path_to_path_in_source_dir(file.path()))
            .map_err(|e| {
                tracing::error!(?e, path = ?file.path(), "failed to get metadata");
                e.raw_os_error().unwrap_or(libc::EIO)
            })
            .and_then(|metadata| {
                self.check_metadata_file_size(file, &metadata)?;
                Ok(metadata)
            })
    }

    fn check_metadata_file_size(&self, file: &V::File, metadata: &Metadata) -> Result<(), i32> {
        // Verify file size if metadata indicates a regular file or symlink
        if metadata.is_file() || metadata.is_symlink() {
            // Only check size for files, not directories
            let expected_size = file.expected_size().ok_or_else(|| {
                tracing::error!(
                    path = ?file.path(),
                    "Cannot get expected size for file (possibly a directory)"
                );
                libc::EIO
            })?;
            let actual_size = metadata.len();
            if actual_size != expected_size {
                tracing::error!(
                    path = ?file.path(),
                    expected_size,
                    actual_size,
                    "File size mismatch"
                );
                return Err(libc::EIO);
            }
        }
        Ok(())
    }

    fn open_file(&self, relative_path: &RelativePath) -> Result<cap_std::fs::File, i32> {
        self.source
            .open(relative_path_to_path_in_source_dir(relative_path))
            .map_err(|e| {
                tracing::error!(?e, ?relative_path, "failed to open file");
                e.raw_os_error().unwrap_or(libc::EIO)
            })
    }

    fn read_file(&self, file: &V::File, offset: i64, size: u32) -> Result<Vec<u8>, i32> {
        if offset < 0 {
            tracing::error!(offset, "invalid offset");
            return Err(libc::EINVAL);
        }
        let requested_offset = offset as u64;
        let requested_size = size as u64;

        // Get block size from file - only files have block size
        let block_size = file.block_size().ok_or_else(|| {
            tracing::error!(
                path = ?file.path(),
                "Cannot read from directory"
            );
            libc::EISDIR
        })? as u64;

        // Calculate aligned offset and size for block reading
        let aligned_offset = (requested_offset / block_size) * block_size;
        let end_offset = requested_offset + requested_size;
        let aligned_end = end_offset.div_ceil(block_size) * block_size;
        let aligned_size = aligned_end - aligned_offset;

        // Calculate starting block index
        let start_block = (aligned_offset / block_size) as usize;

        // Open file and seek to aligned offset
        let mut file_handle = self.open_file(file.path())?;
        file_handle
            .seek(SeekFrom::Start(aligned_offset))
            .map_err(|e| {
                tracing::error!(?e, path = ?file.path(), aligned_offset, "failed to seek file");
                e.raw_os_error().unwrap_or(libc::EIO)
            })?;

        // Read all aligned blocks
        let mut aligned_buf = Vec::with_capacity(aligned_size as usize);
        file_handle
            .take(aligned_size)
            .read_to_end(&mut aligned_buf)
            .map_err(|e| {
                tracing::error!(
                    ?e,
                    path = ?file.path(),
                    aligned_offset,
                    aligned_size,
                    "failed to read file"
                );
                e.raw_os_error().unwrap_or(libc::EIO)
            })?;

        // Verify each block using chunks
        for (chunk_idx, block_data) in aligned_buf.chunks(block_size as usize).enumerate() {
            let block_idx = start_block + chunk_idx;

            if let Err(e) = file.verify_data_block(block_idx, block_data) {
                tracing::error!(
                    ino = file.ino(),
                    path = ?file.path(),
                    block_idx,
                    "Data block verification failed: {}",
                    e
                );
                return Err(libc::EIO);
            }
        }

        // Extract the originally requested data range from aligned buffer
        let start_in_buf = (requested_offset - aligned_offset) as usize;
        let end_in_buf = start_in_buf + requested_size as usize;
        let end_in_buf = end_in_buf.min(aligned_buf.len());

        Ok(aligned_buf[start_in_buf..end_in_buf].to_vec())
    }
    fn read_dir(&self, relative_path: &RelativePath) -> Result<cap_std::fs::ReadDir, i32> {
        self.source
            .read_dir(relative_path_to_path_in_source_dir(relative_path))
            .map_err(|e| {
                tracing::error!(?e, ?relative_path, "failed to read dir");
                e.raw_os_error().unwrap_or(libc::EIO)
            })
    }

    fn read_link(&self, relative_path: &RelativePath) -> Result<PathBuf, i32> {
        self.source
            .read_link(relative_path_to_path_in_source_dir(relative_path))
            .map_err(|e| {
                tracing::error!(?e, ?relative_path, "failed to open file");
                e.raw_os_error().unwrap_or(libc::EIO)
            })
    }
}

fn relative_path_to_path_in_source_dir(relative_path: &RelativePath) -> PathBuf {
    relative_path.to_logical_path(".")
}

fn metadata_to_attr(metadata: &Metadata, ino: u64) -> Result<FileAttr, i32> {
    let file_type = if metadata.is_dir() {
        FileType::Directory
    } else if metadata.is_file() {
        FileType::RegularFile
    } else if metadata.file_type().is_symlink() {
        FileType::Symlink
    } else {
        return Err(libc::EOPNOTSUPP);
    };

    Ok(FileAttr {
        ino,
        size: metadata.len(),
        blocks: (metadata.len() / 512) + 1,
        atime: metadata
            .accessed()
            .map(|t| t.into_std())
            .unwrap_or_else(|_| SystemTime::now()),
        mtime: metadata
            .modified()
            .map(|t| t.into_std())
            .unwrap_or_else(|_| SystemTime::now()),
        ctime: metadata
            .created()
            .map(|t| t.into_std())
            .unwrap_or_else(|_| SystemTime::now()),
        crtime: metadata
            .created()
            .map(|t| t.into_std())
            .unwrap_or_else(|_| SystemTime::now()),
        kind: file_type,
        perm: (metadata.permissions().mode() & 0o777) as u16, // extract permission bits
        nlink: metadata.nlink() as u32,
        uid: metadata.uid(),
        gid: metadata.gid(),
        rdev: 0,
        flags: 0,
        blksize: FS_BLOCK_SIZE,
    })
}

impl<V: FileVerifier> fuser::Filesystem for VerityFS<V> {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        tracing::debug!(parent, ?name, "lookup");

        let result = || -> _ {
            // First lookup parent to get its path
            let parent_file = self.verifier.lookup_by_ino(parent).ok_or(libc::ENOENT)?;
            let parent_path = parent_file.path();
            let child_path =
                parent_path.join(RelativePath::from_path(Path::new(name)).map_err(|e| {
                    tracing::error!(?parent_path, ?name, ?e, "invalid lookup entry name");
                    libc::EINVAL
                })?);

            // Try to lookup child as a verifiable file
            let child_file = self
                .verifier
                .lookup_by_path(&child_path)
                .ok_or(libc::ENOENT)?;

            let metadata = self.get_metadata(child_file)?;
            metadata_to_attr(&metadata, child_file.ino())
        }();

        match result {
            Ok(attr) => {
                reply.entry(&TTL, &attr, 0);
            }
            Err(err) => {
                tracing::error!(?name, err, "lookup failed");
                reply.error(err);
            }
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        tracing::debug!(ino, "getattr");

        let result = || -> _ {
            let file = self.verifier.lookup_by_ino(ino).ok_or(libc::ENOENT)?;
            let metadata = self.get_metadata(file)?;
            metadata_to_attr(&metadata, ino)
        }();

        match result {
            Ok(attr) => reply.attr(&TTL, &attr),
            Err(err) => {
                tracing::error!(ino, "getattr failed");
                reply.error(err);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        tracing::debug!(ino, offset, size, "read");

        let data_result = || -> _ {
            let file = self.verifier.lookup_by_ino(ino).ok_or(libc::EINVAL)?;
            self.read_file(file, offset, size)
        }();

        match data_result {
            Ok(data) => reply.data(&data),
            Err(err) => reply.error(err),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        tracing::debug!(ino, offset, "readdir");

        // TODO: Replace this implementation to return from the recorded file list instead of from the underlying filesystem, so that the returned directory entries are not affected by modifications from the underlying unsafe filesystem.
        let entries_result = || -> _ {
            let file = self.verifier.lookup_by_ino(ino).ok_or(libc::ENOENT)?;
            let dir_path = file.path();
            let metadata = self.get_metadata(file)?;

            if !metadata.is_dir() {
                return Err(libc::ENOTDIR);
            }

            let mut entries = Vec::new();

            // . and ..
            if offset <= 0 {
                entries.push((ino, FileType::Directory, ".".into()));
            }
            if offset <= 1 {
                entries.push((ino, FileType::Directory, "..".into()));
            }

            let dir_iter = self.read_dir(dir_path)?;

            dir_iter.flatten().for_each(|entry| {
                let _ /* just skip and ignore the error */= || -> _ {
                    let name = entry.file_name();
                    let child_path =
                        dir_path.join(RelativePath::from_path(Path::new(&name)).map_err(|e| {
                            tracing::error!(?dir_path, ?name, ?e, "invalid dir entry name");
                        })?);

                    let metadata = entry.metadata().map_err(|e| {
                        tracing::error!(?dir_path, ?name, ?e, "failed to get dir entry metadata");
                    })?;

                    // Try to lookup child
                    let child_file = self.verifier.lookup_by_path(&child_path).ok_or_else(|| {
                        tracing::debug!(?dir_path, ?name, "failed to lookup dir entry, maybe the file was not recorded");
                    })?;
                    self.check_metadata_file_size(child_file, &metadata).map_err(|_e|())?;

                    let child_type = if metadata.is_dir() {
                        FileType::Directory
                    } else if metadata.is_file() {
                        FileType::RegularFile
                    } else if metadata.is_symlink() {
                        FileType::Symlink
                    } else {
                        tracing::warn!(?dir_path, ?name, "unknown dir entry file type");
                        Err(())?
                    };

                    entries.push((child_file.ino(), child_type, name));

                    Ok::<_, ()>(())
                }();
            });

            Ok(entries)
        }();

        let offset = offset.max(0);

        match entries_result {
            Ok(entries) => {
                for (i, (ino, ty, name)) in entries.into_iter().skip(offset as usize).enumerate() {
                    let index = offset + i as i64 + 1;
                    if reply.add(ino, index, ty, name) {
                        break;
                    }
                }
                reply.ok();
            }
            Err(err) => {
                tracing::error!(ino, "readdir failed: {}", err);
                reply.error(err);
            }
        }
    }

    fn open(&mut self, _req: &Request, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        tracing::debug!(ino, "open");

        let result = || -> _ {
            let file = self.verifier.lookup_by_ino(ino).ok_or(libc::ENOENT)?;
            let _ = self.open_file(file.path())?;
            Ok(file)
        }();

        match result {
            Ok(file) => {
                tracing::info!(ino, path=?file.path(), "open");
                reply.opened(0, 0)
            }
            Err(e) => {
                tracing::error!(ino, "open failed");
                reply.error(e);
            }
        }
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        tracing::debug!(ino, "readlink");

        let result = || -> _ {
            let file = self.verifier.lookup_by_ino(ino).ok_or(libc::ENOENT)?;
            self.read_link(file.path())
        }();

        match result {
            Ok(path) => reply.data(path.as_os_str().as_bytes()),
            Err(e) => {
                tracing::error!(ino, "readlink failed");
                reply.error(e);
            }
        }
    }
}
