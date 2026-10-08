use crate::symbol::Symbol;
use crate::types::CheckerTyTable;
use rustc_hash::FxHashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExportMap {
    symbols: FxHashMap<String, Symbol>,
    #[serde(skip)]
    pub table: Arc<CheckerTyTable>,
}

impl ExportMap {
    pub fn with_table(table: Arc<CheckerTyTable>) -> Self {
        ExportMap {
            symbols: FxHashMap::default(),
            table,
        }
    }
}

impl IntoIterator for ExportMap {
    type Item = (String, Symbol);
    type IntoIter = std::collections::hash_map::IntoIter<String, Symbol>;

    fn into_iter(self) -> Self::IntoIter {
        self.symbols.into_iter()
    }
}

impl Deref for ExportMap {
    type Target = FxHashMap<String, Symbol>;

    fn deref(&self) -> &Self::Target {
        &self.symbols
    }
}

impl DerefMut for ExportMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.symbols
    }
}

pub(crate) fn assign_slots(exports: &mut ExportMap) {
    let mut keys: Vec<String> = exports.keys().cloned().collect();
    keys.sort();
    for (idx, key) in keys.iter().enumerate() {
        if let Some(sym) = exports.get_mut(key) {
            sym.slot_idx = Some(idx);
        }
    }
}
