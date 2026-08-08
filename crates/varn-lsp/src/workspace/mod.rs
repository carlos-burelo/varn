pub mod revision;
pub mod std_sources;
use crate::db::{CancellationToken, Database, FileId};
use crate::document::DocumentState;
use crate::index::ProjectIndex;
use crate::pipeline::run_pipeline;
use crate::queries::exports::{ExportedSymbol, ModuleExports};
use dashmap::DashMap;
use std::sync::{Arc, RwLock};

pub use revision::{Cached, Revision};

pub struct Workspace {
    files: DashMap<String, Arc<DocumentState>>,
    exports: DashMap<FileId, ModuleExports>,
    pub db: Arc<Database>,
    pub index: RwLock<ProjectIndex>,
    revision: RwLock<Revision>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            exports: DashMap::new(),
            db: Arc::new(Database::new()),
            index: RwLock::new(ProjectIndex::new()),
            revision: RwLock::new(Revision::new()),
        }
    }

    /// Fast O(1) source text update (<0.5ms). Returns (FileId, Revision, CancellationToken)
    pub fn update_source(&self, uri: &str, source: &str) -> (FileId, u64, CancellationToken) {
        let file_id = self.db.intern(uri);
        let (rev, token) = self.db.set_source(file_id, source.to_string());
        (file_id, rev, token)
    }

    /// Heavy pipeline analysis with Query Firewall check.
    pub fn update_file(&self, uri: String, source: String) {
        varn_checker::module_resolver::invalidate_module_cache();

        let file_id = self.db.intern(&uri);
        // Only store text that is actually new. `set_source` cancels the
        // file's outstanding token, and the caller that ran `update_source`
        // first is holding that very token to detect being superseded — so
        // re-storing identical text here would make every analysis cancel
        // itself before it could publish anything.
        let already_current = self
            .db
            .get_source(file_id)
            .is_some_and(|(_, stored)| *stored == *source);
        if !already_current {
            self.db.set_source(file_id, source.clone());
        }

        let state = Arc::new(run_pipeline(source, uri.clone()));

        let current_exports = extract_exports(&state);
        let exports_changed = match self.exports.get(&file_id) {
            Some(prev) => !current_exports.is_unchanged_from(prev.value()),
            None => true,
        };
        self.exports.insert(file_id, current_exports);

        let dependents: Vec<(String, String)> = if exports_changed {
            let idx = self.index.read().unwrap();
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
            let mut idx = self.index.write().unwrap();
            idx.update_file(&uri, &state);
        }
        self.files.insert(uri.clone(), state);

        for (dep_uri, dep_source) in dependents {
            let dep_path = crate::document::uri_to_path(&dep_uri);
            let dep_canonical = varn_modules::canonical_or_original(std::path::Path::new(&dep_path));
            varn_checker::module_resolver::invalidate_module(&varn_core::ModuleId::local_str(
                &dep_canonical,
            ));
            let dep_state = Arc::new(run_pipeline(dep_source, dep_uri.clone()));
            {
                let mut idx = self.index.write().unwrap();
                idx.update_file(&dep_uri, &dep_state);
            }
            self.files.insert(dep_uri, dep_state);
        }

        {
            let mut rev = self.revision.write().unwrap();
            rev.bump();
        }
    }

    pub fn remove_file(&self, uri: &str) {
        let file_id = self.db.intern(uri);
        self.exports.remove(&file_id);
        self.files.remove(uri);
        let mut idx = self.index.write().unwrap();
        idx.remove_file(uri);
    }

    pub fn get(&self, uri: &str) -> Option<Arc<DocumentState>> {
        self.files.get(uri).map(|r| Arc::clone(r.value()))
    }

    pub fn iter(&self) -> dashmap::iter::Iter<'_, String, Arc<DocumentState>> {
        self.files.iter()
    }

    pub fn revision(&self) -> u32 {
        self.revision.read().unwrap().current()
    }
}

fn extract_exports(state: &DocumentState) -> ModuleExports {
    let mut exported_symbols = Vec::new();
    for sym in &state.symbols {
        if !sym.is_from_stdlib && sym.line != u32::MAX {
            exported_symbols.push(ExportedSymbol {
                name: sym.name.clone(),
                kind_str: format!("{:?}", sym.kind),
                signature_str: format!("{}: {}", sym.type_str, sym.params_str),
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
