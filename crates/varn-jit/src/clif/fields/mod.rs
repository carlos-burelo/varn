//! Fixed-field access lowering for CLIF: `GetFixedField` / `SetFixedField`
//! on compact class fields, shared with the lowering from typed SSA.

mod compact;

pub(crate) use compact::{load_compact, store_compact, FieldIo};
