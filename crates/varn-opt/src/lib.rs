use std::rc::Rc;

use rustc_hash::FxHashMap;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;
use varn_types::FunctionProto;

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
    let mut proto = lower::lower(&module, source_file, export_names)?;
    varn_backend::run_post_passes(&mut proto);
    Ok(proto)
}

pub fn lower_to_hir(input: OptInput<'_>) -> Result<hir::HirModule, OptError> {
    hir::lower::lower_program(&input)
}

pub fn lower_to_ssa(
    input: OptInput<'_>,
) -> Result<(Vec<ssa::ir::SsaFunc>, Vec<(Rc<str>, &'static str)>), OptError> {
    let module = hir::lower::lower_program(&input)?;
    let mut funcs = Vec::new();
    let mut errors: Vec<(Rc<str>, &'static str)> = Vec::new();

    match ssa::build::build_function(&module.top_level, &module.functions) {
        Ok(mut f) => {
            crate::passes::optimize(&mut f);
            funcs.push(f);
        }
        Err(OptError::Unsupported(msg)) => {
            errors.push((module.top_level.name.clone(), msg));
        }
    }

    for f in &module.functions {
        match ssa::build::build_function(f, &[]) {
            Ok(mut sf) => {
                crate::passes::optimize(&mut sf);
                funcs.push(sf);
            }
            Err(OptError::Unsupported(msg)) => {
                errors.push((f.name.clone(), msg));
            }
        }
    }

    Ok((funcs, errors))
}
