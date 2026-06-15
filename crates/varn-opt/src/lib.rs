//! `varn-opt` — the optimizing compiler backend.
//!
//! Pipeline: `AST -> HIR -> SSA -> opt passes -> bytecode (FunctionProto)`.
//! The register-allocation post-pass (`regalloc_post`), the register VM, and
//! the JIT are reused unchanged: this crate lowers to the same `FunctionProto`
//! they already consume.
//!
//! This is being built incrementally behind the `VN_OPT` env flag (see
//! `varn_compiler::codegen::compile_direct`). Until the lowering covers a given
//! program, [`compile`] returns `Err`, and the caller falls back to the legacy
//! direct codegen — so the compiler stays fully functional at every stage.

use std::rc::Rc;

use rustc_hash::FxHashMap;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;
use varn_types::FunctionProto;

pub mod hir;
pub mod lower;
pub mod passes;
pub mod ssa;

/// Inputs threaded from the checker/compiler front-end, mirroring
/// `varn_compiler::codegen::compile_direct`'s signature.
pub struct OptInput<'a> {
    pub program: &'a Program,
    pub annotations: &'a TypeAnnotations,
    pub extension_calls: &'a FxHashMap<u32, Rc<str>>,
    pub extension_members: &'a FxHashMap<u32, Rc<str>>,
    pub extension_set_members: &'a FxHashMap<u32, Rc<str>>,
    pub export_names: Vec<Rc<str>>,
}

/// Why the optimizer declined to compile a program (the caller falls back to
/// the legacy codegen). During bring-up this is overwhelmingly `Unsupported`.
#[derive(Debug)]
pub enum OptError {
    /// A construct the incremental lowering does not handle yet.
    Unsupported(&'static str),
}

/// Compile a whole module through the optimizer pipeline to a top-level
/// `FunctionProto`. Returns `Err(OptError::Unsupported(..))` for anything the
/// current stage cannot lower, signalling the caller to use legacy codegen.
pub fn compile(input: OptInput<'_>) -> Result<FunctionProto, OptError> {
    let source_file = input.program.filename.clone();
    let export_names = input.export_names.clone();
    let module = hir::lower::lower_program(&input)?;
    if std::env::var_os("VN_OPT_TRACE").is_some() {
        eprintln!(
            "[varn-opt] compiled module: {} fn(s) + top-level",
            module.functions.len()
        );
    }
    lower::lower(&module, source_file, export_names)
}
