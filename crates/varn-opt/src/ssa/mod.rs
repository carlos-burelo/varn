use crate::hir::{HirFunction, HirModule};
use crate::OptError;
use std::rc::Rc;
use varn_types::FunctionProto;
pub mod build;
pub mod dump;
pub mod emit;
pub mod ir;
pub mod verify;

pub fn try_compile_function(
    f: &HirFunction,
    source_file: Rc<str>,
) -> Result<FunctionProto, OptError> {
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
