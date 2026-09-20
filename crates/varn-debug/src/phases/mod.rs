//! Migrated phases (DEBUG_PLAN §3): each computes a `Report` and renders it in
//! either `Plain` (byte-identical to the historical `eprintln!` output) or
//! `Text` (diffable).

pub mod cap_trace;
pub mod modules;
pub mod scope;
pub mod tokens;
