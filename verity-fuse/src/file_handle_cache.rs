//! File handle cache for verity-fuse
//!
//! Provides LRU-based caching for file handles to avoid repeated open/close syscalls.

use cap_std::fs::File;
use lru::LruCache;
use std::io::{Error, ErrorKind};
use std::mem::MaybeUninit;
use std::num::NonZeroUsize;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};

const DEFAULT_HANDLE_CACHE_SIZE: usize = 1024;

/// Cached file handle wrapper with pread support
pub struct CachedFileHandle {
    file: File,
}

impl CachedFileHandle {
    pub fn new(file: File) -> Self {
        Self { file }
    }

    pub fn pread_file_data(&self, offset: u64, size: usize) -> std::io::Result<Vec<u8>> {
        let mut buf: Vec<MaybeUninit<u8>> = Vec::with_capacity(size);
        unsafe {
            buf.set_len(size);
        }
        let bytes_read = self.pread_all(&mut buf, offset)?;
        // Truncate buffer to actual bytes read
        unsafe {
            buf.set_len(bytes_read);
        }
        let vec_u8 = unsafe { std::mem::transmute::<Vec<MaybeUninit<u8>>, Vec<u8>>(buf) };
        Ok(vec_u8)
    }

    /// Read bytes at offset using pread() - thread-safe, doesn't change file position
    /// Returns the number of bytes actually read (may be less than buf.len() at EOF)
    fn pread_all(&self, buf: &mut [MaybeUninit<u8>], mut offset: u64) -> std::io::Result<usize> {
        let fd = self.file.as_raw_fd();
        let mut total_read = 0;

        while total_read < buf.len() {
            let ret = unsafe {
                libc::pread(
                    fd,
                    buf[total_read..].as_mut_ptr() as *mut libc::c_void,
                    buf.len() - total_read,
                    offset as libc::off_t,
                )
            };
            match ret.cmp(&0) {
                std::cmp::Ordering::Less => {
                    let err = Error::last_os_error();
                    if err.kind() == ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(err);
                }
                std::cmp::Ordering::Equal => {
                    // EOF reached - return what we have
                    break;
                }
                std::cmp::Ordering::Greater => {
                    total_read += ret as usize;
                    offset += ret as u64;
                }
            }
        }
        Ok(total_read)
    }
}

/// LRU cache for file handles, keyed by ino
pub struct FileHandleCache {
    cache: Mutex<LruCache<u64, Arc<CachedFileHandle>>>,
}

impl FileHandleCache {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_HANDLE_CACHE_SIZE)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN),
            )),
        }
    }

    /// Get cached handle or open new file using provided opener function
    pub fn get_or_open<F>(&self, ino: u64, opener: F) -> std::io::Result<Arc<CachedFileHandle>>
    where
        F: FnOnce() -> std::io::Result<File>,
    {
        let mut cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.get(&ino) {
            return Ok(Arc::clone(cached));
        }
        let file = opener()?;
        let cached = Arc::new(CachedFileHandle::new(file));
        cache.put(ino, Arc::clone(&cached));
        Ok(cached)
    }
}

impl Default for FileHandleCache {
    fn default() -> Self {
        Self::new()
    }
}
