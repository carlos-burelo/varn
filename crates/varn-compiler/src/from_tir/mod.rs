//! `varn_tir::TirModule` -> SSA, the stage-3 reattachment point.
//!
//! Runs parallel to `ssa/build/` (the HIR path) until it covers the whole
//! corpus; then the pipeline switches to it and HIR is deleted. See
//! `docs/TIR_ETAPA_3_PLAN.md`.
//!
//! Nothing is wired yet: the pipeline still compiles through HIR, so the
//! corpus stays green while this fills in.

use crate::ssa::ir::SsaFunc;
use crate::OptError;
use varn_tir::TirModule;

/// Build one SSA function per `TirFunction` in the module (top level first).
/// Currently unimplemented — the translation is ported construct by construct
/// in step 3.3 of the plan.
pub fn build_module(_tir: &TirModule) -> Result<Vec<SsaFunc>, OptError> {
    Err(OptError::Unsupported("from_tir: not yet implemented"))
}
