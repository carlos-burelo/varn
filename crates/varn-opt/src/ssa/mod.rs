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

use crate::hir::{HirFunction, HirModule};
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
    let mut ssa = build::build_function(f, &[])?;
    crate::passes::optimize(&mut ssa);
    if let Err(why) = verify::verify(&ssa) {
        if std::env::var_os("VN_OPT_TRACE").is_some() {
            eprintln!("[varn-opt] ssa verification failed for {}: {}", f.name, why);
        }
        return Err(OptError::Unsupported("ssa: verify failed"));
    }
    emit::emit_function(ssa, f, source_file)
}

pub fn lower_module(
    module: &HirModule,
    source_file: Rc<str>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, OptError> {
    let mut ssa = build::build_function(&module.top_level, &module.functions)?;
    crate::passes::optimize(&mut ssa);
    if let Err(why) = verify::verify(&ssa) {
        if std::env::var_os("VN_OPT_TRACE").is_some() {
            eprintln!("[varn-opt] ssa verification failed for top-level: {}", why);
        }
        return Err(OptError::Unsupported("ssa: verify failed"));
    }
    let mut proto = emit::emit_function(ssa, &module.top_level, source_file)?;
    proto.export_names = export_names;
    Ok(proto)
}
