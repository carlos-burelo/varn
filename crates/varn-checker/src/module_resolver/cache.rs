//! Formato en disco de la interfaz de un módulo.
//!
//! Nada de lo que cruza esta frontera puede llevar un `CheckerTyId`: un `Type`
//! solo significa algo en la `CheckerTyTable` que lo internó, y el proceso que
//! escribe el caché no es el que lo lee. Por eso todo tipo viaja como
//! [`PortableType`](crate::types::PortableType) (nombres + estructura, sin ids)
//! y se re-interna en la tabla del lector.

mod cache_decode;
mod cache_encode;
mod cache_io;
mod cache_types;

pub use cache_io::{deserialize_module_interface, serialize_module_interface};
pub(crate) use cache_decode::decode_symbol;
pub(crate) use cache_encode::encode_symbol;
pub(crate) use cache_io::{save_to_cache, try_load_cache};
pub type ExportMap = FxHashMap<String, Symbol>;

use rustc_hash::FxHashMap;
use crate::symbol::Symbol;
