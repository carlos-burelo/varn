pub mod builder;
use crate::document::DocumentState;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use varn_sem::symbol::SymbolKind;

#[derive(Debug, Clone)]
pub struct ExportEntry {
    pub name: String,
    pub name_lower: String,
    pub kind: SymbolKind,

    pub uri: Arc<str>,

    pub parent: Option<Arc<str>>,
    pub line: u32,
    pub col: u32,
    pub type_str: String,
    pub doc: Option<String>,
}

pub struct ProjectIndex {
    pub module_exports: BTreeMap<String, Vec<Arc<ExportEntry>>>,
    pub name_index: BTreeMap<String, Vec<Arc<ExportEntry>>>,
    pub reverse_deps: BTreeMap<String, BTreeSet<String>>,
    pub module_cache: BTreeMap<String, String>,
}

impl ProjectIndex {
    pub fn new() -> Self {
        Self {
            module_exports: BTreeMap::new(),
            name_index: BTreeMap::new(),
            reverse_deps: BTreeMap::new(),
            module_cache: BTreeMap::new(),
        }
    }

    pub fn update_file(&mut self, uri: &str, state: &DocumentState) {
        if !self.module_exports.contains_key(uri) {
            builder::index_file(self, uri, state);
            return;
        }
        self.remove_file(uri);
        builder::index_file(self, uri, state);
    }

    pub fn remove_file(&mut self, uri: &str) {
        self.module_exports.remove(uri);
        for entries in self.name_index.values_mut() {
            entries.retain(|e| e.uri.as_ref() != uri);
        }
        self.name_index.retain(|_, v| !v.is_empty());

        for dependents in self.reverse_deps.values_mut() {
            dependents.remove(uri);
        }

        self.reverse_deps.remove(uri);

        self.module_cache.retain(|_, v| v != uri);
    }

    pub fn definitions_of(&self, name: &str) -> &[Arc<ExportEntry>] {
        self.name_index.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn dependents_of(&self, uri: &str) -> impl Iterator<Item = &str> {
        self.reverse_deps
            .get(uri)
            .into_iter()
            .flat_map(|s| s.iter().map(String::as_str))
    }
}

impl Default for ProjectIndex {
    fn default() -> Self {
        Self::new()
    }
}
