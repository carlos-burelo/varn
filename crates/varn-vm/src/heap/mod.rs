//! The heap: object storage, the two generations, and everything that puts a
//! value into a slot or reads one back out.
//!
//! `alloc.rs` used to be all of that in one 830-line file. It is now split by
//! what is being allocated or read; the methods are still inherent methods on
//! `HeapInner`, since Rust lets one inherent impl span modules of a crate, so
//! nothing outside this directory had to change.

pub(crate) mod access;
pub(crate) mod aggregates;
pub(crate) mod core;
pub(crate) mod gc;
pub(crate) mod intern;
pub(crate) mod jit;
pub(crate) mod map_keys;
pub(crate) mod native;
pub(crate) mod obj;
pub(crate) mod str;
pub(crate) mod strings;
pub(crate) mod structs;
pub(crate) mod values;

pub use obj::*;
pub use str::*;
pub use structs::*;
