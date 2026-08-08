use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

pub mod cancel;
pub use cancel::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

impl FileId {
    pub const NULL: FileId = FileId(u32::MAX);
}

#[derive(Default)]
pub struct FileInterner {
    map: DashMap<String, FileId>,
    vec: RwLock<Vec<String>>,
}

impl FileInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&self, uri: &str) -> FileId {
        if let Some(id) = self.map.get(uri) {
            return *id.value();
        }

        let mut vec = self.vec.write().unwrap();
        // Double check after lock
        if let Some(id) = self.map.get(uri) {
            return *id.value();
        }

        let id = FileId(vec.len() as u32);
        vec.push(uri.to_string());
        self.map.insert(uri.to_string(), id);
        id
    }

    pub fn lookup(&self, id: FileId) -> Option<String> {
        let vec = self.vec.read().unwrap();
        vec.get(id.0 as usize).cloned()
    }
}

pub struct Database {
    interner: FileInterner,
    revision: AtomicU64,
    sources: DashMap<FileId, (u64, Arc<str>)>,
    cancellation_tokens: DashMap<FileId, CancellationToken>,
}

impl Database {
    pub fn new() -> Self {
        Self {
            interner: FileInterner::new(),
            revision: AtomicU64::new(1),
            sources: DashMap::new(),
            cancellation_tokens: DashMap::new(),
        }
    }

    pub fn intern(&self, uri: &str) -> FileId {
        self.interner.intern(uri)
    }

    pub fn lookup(&self, id: FileId) -> Option<String> {
        self.interner.lookup(id)
    }

    pub fn set_source(&self, file_id: FileId, source: String) -> (u64, CancellationToken) {
        let rev = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.sources.insert(file_id, (rev, Arc::from(source)));

        // Cancel previous token for this file if any
        if let Some(old_token) = self.cancellation_tokens.get(&file_id) {
            old_token.cancel();
        }

        let new_token = CancellationToken::new();
        self.cancellation_tokens.insert(file_id, new_token.clone());
        (rev, new_token)
    }

    pub fn get_source(&self, file_id: FileId) -> Option<(u64, Arc<str>)> {
        self.sources.get(&file_id).map(|r| r.value().clone())
    }

    pub fn cancellation_token(&self, file_id: FileId) -> Option<CancellationToken> {
        self.cancellation_tokens
            .get(&file_id)
            .map(|r| r.value().clone())
    }

    pub fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}
