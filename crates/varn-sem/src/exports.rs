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
    pub collisions: Vec<ExportCollision>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExportCollision {
    pub name: String,
    pub kept_from: String,
    pub dropped_from: String,
}

impl ExportMap {
    pub fn with_table(table: Arc<CheckerTyTable>) -> Self {
        ExportMap {
            symbols: FxHashMap::default(),
            table,
            collisions: Vec::new(),
        }
    }

    pub fn record_collision(&mut self, name: String, kept_from: String, dropped_from: String) {
        if self
            .collisions
            .iter()
            .all(|c| c.name != name || c.dropped_from != dropped_from)
        {
            self.collisions.push(ExportCollision {
                name,
                kept_from,
                dropped_from,
            });
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
