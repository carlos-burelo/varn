//! The heap: object storage, the two generations, and everything that puts a
//! value into a slot or reads one back out.
//!
//! `alloc.rs` used to be all of that in one 830-line file. It is now split by
//! what is being allocated or read; the methods are still inherent methods on
//! `HeapInner`, since Rust lets one inherent impl span modules of a crate, so
//! nothing outside this directory had to change.

pub mod access;
pub mod aggregates;
pub mod core;
pub mod gc;
pub mod intern;
pub mod jit;
pub mod map_keys;
pub mod native;
pub mod obj;
pub mod str;
pub mod strings;
pub mod structs;
pub mod values;

pub use obj::*;
pub use str::*;
pub use structs::*;
