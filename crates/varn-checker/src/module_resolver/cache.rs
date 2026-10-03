//! Formato en disco de la interfaz de un módulo: símbolos y tipos tal cual
//! (ids por contenido), más la porción de tabla y los nombres que alcanzan.

mod cache_decode;
mod cache_encode;
mod cache_io;
mod cache_types;

pub use cache_io::{deserialize_module_interface, serialize_module_interface};
pub(crate) use cache_io::{save_to_cache, try_load_cache};

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
