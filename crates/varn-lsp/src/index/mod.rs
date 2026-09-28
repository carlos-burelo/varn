pub mod builder;
use crate::document::DocumentState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use varn_checker::SymbolKind;

#[derive(Debug, Clone)]
pub struct ExportEntry {
    pub name: String,
    pub kind: SymbolKind,
    /// Un solo `Arc<str>` por archivo, compartido por sus N entradas: el uri
    /// como `String` por entrada costaba N×len(uri) (H4).
    pub uri: Arc<str>,
    /// Owner type name for member entries; `None` for top-level symbols.
    /// Replaces the formatted `member:{parent}:{member}` global key: member
    /// identity is structural now, compared field-wise, never rendered.
    pub parent: Option<Arc<str>>,
    pub line: u32,
    pub col: u32,
    pub type_str: String,
    pub doc: Option<String>,
}

pub struct ProjectIndex {
    pub module_exports: HashMap<String, Vec<Arc<ExportEntry>>>,
    pub name_index: HashMap<String, Vec<Arc<ExportEntry>>>,
    pub reverse_deps: HashMap<String, HashSet<String>>,
    pub module_cache: HashMap<String, String>,
}

impl ProjectIndex {
    pub fn new() -> Self {
        Self {
            module_exports: HashMap::new(),
            name_index: HashMap::new(),
            reverse_deps: HashMap::new(),
            module_cache: HashMap::new(),
        }
    }

    pub fn update_file(&mut self, uri: &str, state: &DocumentState) {
        // Fast path del scan inicial: cada archivo se indexa una vez y no hay
        // nada que evictar. `remove_file` barre `name_index/reverse_deps`
        // completos (O(total) por archivo → O(N²) en el workspace), así que
        // saltarlo cuando el módulo es nuevo cambia el startup de cuadrático
        // a lineal. Las tres tablas se escriben juntas, luego ausencia en
        // `module_exports` implica ausencia en el resto.
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
