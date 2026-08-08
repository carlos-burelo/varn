use crate::hir::{HirFunction, HirModule};
use crate::OptError;
use std::rc::Rc;
use varn_types::FunctionProto;
pub mod build;
pub mod dump;
pub mod emit;
pub mod ir;
pub mod uses;
pub mod verify;

pub fn try_compile_function(
    f: &HirFunction,
    source_file: Rc<str>,
) -> Result<FunctionProto, OptError> {
    let mut ssa = build::build_function(f, &[], Some(source_file.clone()))?;
    crate::passes::optimize_with(&mut ssa, &crate::hir::ctor_summary::current());
    if let Err(why) = verify::verify(&ssa) {
        panic!("ssa: verify failed for {}: {}", f.name, why);
    }
    emit::emit_function(ssa, f, source_file)
}

pub fn lower_module(
    module: &HirModule,
    source_file: Rc<str>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, OptError> {
    let mut ssa = build::build_function(
        &module.top_level,
        &module.functions,
        Some(source_file.clone()),
    )?;
    crate::passes::optimize_with(&mut ssa, &crate::hir::ctor_summary::current());
    if let Err(why) = verify::verify(&ssa) {
        panic!("ssa: verify failed for top-level: {}", why);
    }
    let mut proto = emit::emit_function(ssa, &module.top_level, source_file)?;
    proto.export_names = export_names;
    Ok(proto)
}
