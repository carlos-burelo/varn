use std::rc::Rc;

use rustc_hash::FxHashMap;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

pub use varn_types::chunk::{Chunk, FunctionProto, LineMapping, Literal, PoolEntry};

/// Varn core AST-to-bytecode optimization and compilation pipeline.
pub mod hir;
pub mod lower;
pub mod passes;
pub mod ssa;

pub struct OptInput<'a> {
    pub program: &'a Program,
    pub annotations: &'a TypeAnnotations,
    pub extension_calls: &'a FxHashMap<u32, Rc<str>>,
    pub extension_members: &'a FxHashMap<u32, Rc<str>>,
    pub extension_set_members: &'a FxHashMap<u32, Rc<str>>,
    pub export_names: Vec<Rc<str>>,
}

#[derive(Debug)]
pub enum OptError {
    Unsupported(&'static str),
}

/// Ergonomic module-compilation entry point: builds [`OptInput`], compiles, and
/// maps the backend error to a displayable string. This is the front door the
/// pipeline/CLI use (previously the whole point of the varn-compiler shim).
#[allow(clippy::too_many_arguments)]
pub fn compile_module(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, Rc<str>> {
    compile(OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names,
    })
    .map_err(|e| -> Rc<str> { Rc::from(format!("varn-compiler could not lower module: {e:?}")) })
}

pub fn compile(input: OptInput<'_>) -> Result<FunctionProto, OptError> {
    let source_file = input.program.filename.clone();
    let export_names = input.export_names.clone();
    let mut module = hir::lower::lower_program(&input)?;
    hir::inline::run(&mut module);
    hir::module_locals::run(&mut module);
    if std::env::var_os("VN_OPT_TRACE").is_some() {
        eprintln!(
            "[varn-compiler] compiled module: {} fn(s) + top-level",
            module.functions.len()
        );
    }
    let mut proto = lower::lower(&module, source_file, export_names)?;
    varn_regalloc::run_post_passes(&mut proto);
    Ok(proto)
}

pub fn lower_to_hir(input: OptInput<'_>) -> Result<hir::HirModule, OptError> {
    hir::lower::lower_program(&input)
}

pub fn lower_to_ssa(
    input: OptInput<'_>,
) -> Result<(Vec<ssa::ir::SsaFunc>, Vec<(Rc<str>, &'static str)>), OptError> {
    let mut module = hir::lower::lower_program(&input)?;
    hir::inline::run(&mut module);
    hir::module_locals::run(&mut module);
    let _summaries = hir::ctor_summary::Scope::enter(hir::ctor_summary::collect(&module));
    let mut funcs = Vec::new();
    let mut errors: Vec<(Rc<str>, &'static str)> = Vec::new();

    let source_file = Some(input.program.filename.clone());
    match ssa::build::build_function(&module.top_level, &module.functions, source_file.clone()) {
        Ok(mut f) => {
            crate::passes::optimize_with(&mut f, &hir::ctor_summary::current());
            crate::passes::state_machine::run(&mut f);
            funcs.push(f);
        }
        Err(OptError::Unsupported(msg)) => {
            errors.push((module.top_level.name.clone(), msg));
        }
    }

    for f in &module.functions {
        match ssa::build::build_function(f, &[], source_file.clone()) {
            Ok(mut sf) => {
                crate::passes::optimize_with(&mut sf, &hir::ctor_summary::current());
                crate::passes::state_machine::run(&mut sf);
                funcs.push(sf);
            }
            Err(OptError::Unsupported(msg)) => {
                errors.push((f.name.clone(), msg));
            }
        }
    }

    Ok((funcs, errors))
}
