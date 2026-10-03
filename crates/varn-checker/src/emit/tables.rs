//! The module tables: classes with their layout and vtable, enums, and the
//! signatures both reference. Built before any body, because `Class(ClassId)`
//! and `Enum(EnumId)` need the handle.

mod classes;
mod enums;
mod index;
mod signatures;

pub use index::{build, NameIndex, Tables};
pub(crate) use signatures::intern_signature;
