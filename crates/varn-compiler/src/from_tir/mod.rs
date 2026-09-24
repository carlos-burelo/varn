//! `varn_tir::TirModule` -> SSA, the compiler's only frontend path.
//!
//! Covers the whole corpus; the pipeline compiles through it (HIR was deleted
//! at the cut). See `docs/TIR_CONTRATO_TIPADO.md`.

pub mod build;
pub mod compile;
pub(crate) mod ctor_summary;
pub mod ty;

pub use build::{build_function, build_module};
pub use compile::compile_module;
