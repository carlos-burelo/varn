//! SSA construction and representation.
//!
//! Stage 2: build a per-function CFG of basic blocks in SSA form from HIR, run
//! opt passes (`crate::passes`) on it, then lower back to bytecode. The
//! representation uses **block parameters** instead of phi nodes and is built
//! on-the-fly (Braun et al. 2013); a dominator tree is computed separately for
//! the verifier and passes that need it.

#![allow(dead_code)]

use std::rc::Rc;

use varn_types::FunctionProto;

use crate::hir::HirFunction;
use crate::OptError;

pub mod build;
pub mod dump;
pub mod emit;
pub mod ir;
pub mod verify;

#[cfg(test)]
mod tests;

/// Compile a single HIR function through the SSA pipeline (build → verify →
/// out-of-SSA → bytecode). Returns `Err(Unsupported)` for any construct SSA
/// construction/emission does not yet cover, so the caller falls back to the
/// naive HIR→bytecode path.
pub fn try_compile_function(f: &HirFunction, source_file: Rc<str>) -> Result<FunctionProto, OptError> {
    let ssa = build::build_function(f)?;
    verify::verify(&ssa).map_err(|_| OptError::Unsupported("ssa: verify failed"))?;
    emit::emit_function(ssa, f, source_file)
}
