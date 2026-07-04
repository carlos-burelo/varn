pub mod revision;
use crate::document::DocumentState;
use crate::index::ProjectIndex;
use crate::pipeline::run_pipeline;
use dashmap::DashMap;
use std::sync::{Arc, RwLock};

pub use revision::{Cached, Revision};

pub struct Workspace {
    files: DashMap<String, Arc<DocumentState>>,
    pub index: RwLock<ProjectIndex>,
    revision: RwLock<Revision>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            files: DashMap::new(),
            index: RwLock::new(ProjectIndex::new()),
            revision: RwLock::new(Revision::new()),
        }
    }

    pub fn update_file(&self, uri: String, source: String) {
        let path = crate::document::uri_to_path(&uri);
        let canonical = varn_modules::canonical_or_original(std::path::Path::new(&path));
        varn_checker::module_resolver::invalidate_module(&varn_core::ModuleId::local_str(
            &canonical,
        ));

        let state = Arc::new(run_pipeline(source, uri.clone()));

        let dependents: Vec<(String, String)> = {
            let idx = self.index.read().unwrap();
            idx.dependents_of(&uri)
                .filter_map(|dep_uri: &str| {
                    self.files
                        .get(dep_uri)
                        .map(|s| (dep_uri.to_owned(), s.source.clone()))
                })
                .collect()
        };

        {
            let mut idx = self.index.write().unwrap();
            idx.update_file(&uri, &state);
        }
        self.files.insert(uri, state);

        for (dep_uri, dep_source) in dependents {
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

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
