#![allow(clippy::arc_with_non_send_sync)]

pub mod revision;
pub mod std_sources;
use crate::db::{CancellationToken, Database, FileId};
use crate::document::DocumentState;
use crate::index::ProjectIndex;
use crate::pipeline::run_pipeline;
use crate::query::exports::{ExportedSymbol, ModuleExports};
use dashmap::DashMap;
use std::sync::{Arc, RwLock};
use varn_resolver::DiskResolver;

pub use revision::{Cached, Revision};

pub struct Workspace {
    files: DashMap<String, Arc<DocumentState>>,
    exports: DashMap<FileId, ModuleExports>,
    pub db: Arc<Database>,
    pub index: RwLock<ProjectIndex>,
    revision: RwLock<Revision>,
    resolver: Arc<DiskResolver>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            exports: DashMap::new(),
            db: Arc::new(Database::new()),
            index: RwLock::new(ProjectIndex::new()),
            revision: RwLock::new(Revision::new()),
            resolver: Arc::new(DiskResolver::new()),
        }
    }

    pub fn resolver(&self) -> &DiskResolver {
        &self.resolver
    }

    pub fn resolver_handle(&self) -> Arc<DiskResolver> {
        Arc::clone(&self.resolver)
    }

    pub fn invalidate(&self, id: &varn_core::ModuleId) {
        self.resolver.invalidate(id);
    }

    pub fn update_source(&self, uri: &str, source: &str) -> (FileId, u64, CancellationToken) {
        let file_id = self.db.intern(uri);
        let (rev, token) = self.db.set_source(file_id, source.to_string());
        (file_id, rev, token)
    }

    pub fn source_of(&self, uri: &str) -> Option<String> {
        let file_id = self.db.file_id(uri)?;
        self.db
            .get_source(file_id)
            .map(|(_, text)| text.to_string())
    }

    pub fn update_file(&self, uri: String, source: String) {
        if let Some(existing) = self.files.get(&uri) {
            if existing.source == source {
                return;
            }
        }

        self.invalidate(&Self::module_id_of(&uri));

        let file_id = self.db.intern(&uri);

        let already_current = self
            .db
            .get_source(file_id)
            .is_some_and(|(_, stored)| *stored == *source);
        if !already_current {
            self.db.set_source(file_id, source.clone());
        }

        let state = Arc::new(run_pipeline(source, uri.clone(), self.resolver_handle()));

        let current_exports = extract_exports(&state);
        let exports_changed = match self.exports.get(&file_id) {
            Some(prev) => !current_exports.is_unchanged_from(prev.value()),
            None => true,
        };
        self.exports.insert(file_id, current_exports);

        let dependents: Vec<(String, String)> = if exports_changed {
            let idx = self.index.read().unwrap_or_else(|e| e.into_inner());
            idx.dependents_of(&uri)
                .filter_map(|dep_uri: &str| {
                    self.files
                        .get(dep_uri)
                        .map(|s| (dep_uri.to_owned(), s.source.clone()))
                })
                .collect()
        } else {
            Vec::new()
        };

        {
            let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
            idx.update_file(&uri, &state);
        }
        self.files.insert(uri.clone(), state);

        for (dep_uri, dep_source) in dependents {
            let dep_state = Arc::new(run_pipeline(
                dep_source,
                dep_uri.clone(),
                self.resolver_handle(),
            ));
            {
                let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
                idx.update_file(&dep_uri, &dep_state);
            }
            self.files.insert(dep_uri, dep_state);
        }

        {
            let mut rev = self.revision.write().unwrap_or_else(|e| e.into_inner());
            rev.bump();
        }
    }

    fn module_id_of(uri: &str) -> varn_core::ModuleId {
        let path = crate::document::uri_to_path(uri);
        let canonical = varn_modules::canonical_or_original(std::path::Path::new(&path));
        varn_core::ModuleId::local_str(&canonical)
    }

    pub fn index_file(&self, uri: String, source: String) {
        self.invalidate(&Self::module_id_of(&uri));
        let file_id = self.db.intern(&uri);
        let state = run_pipeline(source, uri.clone(), self.resolver_handle());
        let current_exports = extract_exports(&state);
        self.exports.insert(file_id, current_exports);
        {
            let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
            idx.update_file(&uri, &state);
        }
    }

    pub fn close_file(&self, uri: &str) {
        self.files.remove(uri);
    }

    pub fn remove_file(&self, uri: &str) {
        self.invalidate(&Self::module_id_of(uri));
        if let Some(file_id) = self.db.file_id(uri) {
            self.exports.remove(&file_id);
        }
        self.files.remove(uri);
        let mut idx = self.index.write().unwrap_or_else(|e| e.into_inner());
        idx.remove_file(uri);
    }

    pub fn get(&self, uri: &str) -> Option<Arc<DocumentState>> {
        self.files.get(uri).map(|r| Arc::clone(r.value()))
    }

    pub fn get_fresh(&self, uri: &str) -> Option<Arc<DocumentState>> {
        if let Some(source) = self.source_of(uri) {
            self.update_file(uri.to_owned(), source);
        }
        self.get(uri)
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Arc<DocumentState>> {
        self.files.iter()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn revision(&self) -> u32 {
        self.revision
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .current()
    }
}

fn extract_exports(state: &DocumentState) -> ModuleExports {
    let mut exported_symbols = Vec::new();
    for sym in state.symbols() {
        if !sym.is_from_stdlib() {
            exported_symbols.push(ExportedSymbol {
                name: sym.name().to_owned(),
                kind_str: format!("{:?}", sym.kind()),
                signature_str: format!("{}: {}", sym.type_str(), sym.params_str()),
            });
        }
    }
    ModuleExports::build(exported_symbols)
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
