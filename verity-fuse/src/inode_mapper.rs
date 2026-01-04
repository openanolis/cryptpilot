use relative_path::{RelativePath, RelativePathBuf};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

pub const ROOT_INO: u64 = 1;
pub const ROOT_PATH: &str = "";

#[derive(Debug)]
struct InodeMapperInner {
    next_ino: u64,
    path_to_ino: HashMap<RelativePathBuf, u64>,
    ino_to_path: HashMap<u64, RelativePathBuf>,
}

impl InodeMapperInner {
    fn new() -> Self {
        let mut mapper = Self {
            next_ino: ROOT_INO + 1, // 1 is root
            path_to_ino: HashMap::new(),
            ino_to_path: HashMap::new(),
        };
        // Allocate root
        mapper
            .ino_to_path
            .insert(ROOT_INO, RelativePathBuf::from(ROOT_PATH));
        mapper
            .path_to_ino
            .insert(RelativePathBuf::from(ROOT_PATH), ROOT_INO);
        mapper
    }

    pub fn get_or_insert(&mut self, path: &RelativePath) -> u64 {
        if let Some(&ino) = self.path_to_ino.get(path) {
            return ino;
        }

        let ino = self.next_ino;
        self.next_ino += 1;

        self.path_to_ino.insert(path.to_owned(), ino);
        self.ino_to_path.insert(ino, path.to_owned());

        info!(ino, ?path, "insert entry to inode mapper");
        ino
    }

    pub fn lookup_ino(&self, path: &RelativePath) -> Option<u64> {
        self.path_to_ino.get(path).copied()
    }

    pub fn lookup_path(&self, ino: u64) -> Option<RelativePathBuf> {
        self.ino_to_path.get(&ino).cloned()
    }
}

#[derive(Clone, Debug)]
pub struct InodeMapper(Arc<Mutex<InodeMapperInner>>);

impl Default for InodeMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl InodeMapper {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(InodeMapperInner::new())))
    }

    pub fn lookup_ino(&self, path: &RelativePath) -> Option<u64> {
        let guard = self.0.lock().unwrap();
        guard.lookup_ino(path)
    }

    pub fn lookup_path(&self, ino: u64) -> Option<RelativePathBuf> {
        let guard = self.0.lock().unwrap();
        guard.lookup_path(ino)
    }

    pub fn get_or_insert(&self, path: &RelativePath) -> u64 {
        let mut guard = self.0.lock().unwrap();
        guard.get_or_insert(path)
    }
}
