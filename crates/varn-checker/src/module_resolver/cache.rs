//! Formato en disco de la interfaz de un módulo: símbolos y tipos tal cual
//! (ids por contenido), más la porción de tabla y los nombres que alcanzan.

mod cache_decode;
mod cache_encode;
mod cache_io;
mod cache_types;

pub use cache_io::{deserialize_module_interface, serialize_module_interface};
pub(crate) use cache_io::{save_to_cache, try_load_cache};
pub type ExportMap = FxHashMap<String, Symbol>;

use crate::symbol::Symbol;
use rustc_hash::FxHashMap;
