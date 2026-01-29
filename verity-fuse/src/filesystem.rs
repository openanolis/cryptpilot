use crate::file_handle_cache::{BlockCache, BlockKey, CachedFileHandle, FileHandleCache};
use crate::file_verifier::{FileVerifier, VerifiableFile};
use cap_std::fs::{Dir, Metadata};
use cap_std::fs::{MetadataExt as _, PermissionsExt};
use fuser::{FileAttr, FileType, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc;
use relative_path::RelativePath;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const TTL: Duration = Duration::from_secs(1);
const FS_BLOCK_SIZE: u32 = 4096;

pub struct VerityFS<V: FileVerifier> {
    source: Dir,
    verifier: Arc<V>,
    block_cache: BlockCache,
    handle_cache: FileHandleCache,
}

impl<V: FileVerifier> VerityFS<V> {
    pub fn new(source: &Path, verifier: V) -> anyhow::Result<Self> {
        let dir: Dir = Dir::open_ambient_dir(source, cap_std::ambient_authority())?;
        let verifier = Arc::new(verifier);

        Ok(Self {
            source: dir,
            verifier,
            block_cache: BlockCache::new(),
            handle_cache: FileHandleCache::new(),
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

    fn open_file_cached(&self, file: &V::File) -> Result<Arc<CachedFileHandle>, i32> {
        self.handle_cache
            .get_or_open(file.ino(), || {
                self.source
                    .open(relative_path_to_path_in_source_dir(file.path()))
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            })
            .map_err(|e| {
                tracing::error!(?e, path = ?file.path(), "failed to open file");
                e.raw_os_error().unwrap_or(libc::EIO)
            })
    }

    fn read_file(&self, file: &V::File, offset: i64, size: u32) -> Result<Vec<u8>, i32> {
        if offset < 0 {
            tracing::error!(offset, "invalid offset");
            return Err(libc::EINVAL);
        }
        if size == 0 {
            return Ok(Vec::new());
        }

        let requested_offset = offset as u64;
        let requested_size = size as u64;
        let requested_end = requested_offset + requested_size;

        let path = file.path();
        let ino = file.ino();

        // Get block size — directories don't have one
        let block_size = file.block_size().ok_or_else(|| {
            tracing::error!(?path, "Cannot read from directory");
            libc::EISDIR
        })? as u64;

        if block_size == 0 {
            tracing::error!(?path, "Block size is zero");
            return Err(libc::EINVAL);
        }

        // Compute which blocks overlap with the requested range
        let start_block = (requested_offset / block_size) as usize;
        let end_block = ((requested_end + block_size - 1) / block_size) as usize; // ceil(requested_end / block_size)
        let num_blocks = end_block - start_block;

        let cached_file = self.open_file_cached(file)?;

        // Pre-allocate output buffer with exact size
        let mut output = Vec::with_capacity(requested_size as usize);

        for block_idx in start_block..(start_block + num_blocks) {
            let key = BlockKey::new(ino, block_idx);
            let block_offset = (block_idx as u64) * block_size;
            let block_end = block_offset + block_size;

            // Determine overlap between [block_offset, block_end) and [requested_offset, requested_end)
            let copy_start_in_file = requested_offset.max(block_offset);
            let copy_end_in_file = requested_end.min(block_end);

            // No overlap? (shouldn't happen due to block range calc, but safe)
            if copy_start_in_file >= copy_end_in_file {
                continue;
            }

            // Get or read the full block (for verification and caching)
            let block_data = if let Some(cached) = self.block_cache.get(&key) {
                cached
            } else {
                let raw_block = cached_file
                    .pread_file_data(block_offset, block_size as usize)
                    .map_err(|e| {
                        tracing::error!(
                            error = ?e,
                            ?path,
                            block_idx,
                            "failed to pread block"
                        );
                        e.raw_os_error().unwrap_or(libc::EIO)
                    })?;

                // Verify integrity
                if let Err(e) = file.verify_data_block(block_idx, &raw_block) {
                    tracing::error!(
                        ino,
                        ?path,
                        block_idx,
                        error = %e,
                        "Data block verification failed"
                    );
                    return Err(libc::EIO);
                }

                let arc_block = Arc::from(raw_block); // Arc<[u8]> is more idiomatic than Arc<Vec<u8>>
                self.block_cache.put(key, Arc::clone(&arc_block));
                arc_block
            };

            // Compute slice within the block to copy
            let src_start = (copy_start_in_file - block_offset) as usize;
            let src_end = (copy_end_in_file - block_offset) as usize;

            // Append only the needed part to output
            output.extend_from_slice(&block_data[src_start..src_end]);
        }

        // Safety: we should have filled exactly `requested_size` bytes
        // But be defensive in case of short reads (e.g., file truncated)
        if output.len() != requested_size as usize {
            tracing::warn!(
                ?path,
                expected = requested_size,
                actual = output.len(),
                "Short read: file may be truncated"
            );
            // Optionally: pad with zeros or return error?
            // Here we just return what we got (common in filesystems)
        }

        Ok(output)
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
            let _ = self.open_file_cached(file)?;
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
