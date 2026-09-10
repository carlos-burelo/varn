//! `varn_tir::TirModule` -> SSA, the stage-3 reattachment point.
//!
//! Runs parallel to `ssa/build/` (the HIR path) until it covers the whole
//! corpus; then the pipeline switches to it and HIR is deleted. See
//! `docs/TIR_ETAPA_3_PLAN.md`.
//!
//! Nothing is wired yet: the pipeline still compiles through HIR, so the
//! corpus stays green while this fills in.

pub mod build;
pub mod compile;
pub(crate) mod ctor_summary;
pub mod ty;

pub use build::{build_function, build_module};
pub use compile::compile_module;
