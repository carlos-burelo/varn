use crate::FunctionProto;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

pub fn compile_direct(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, Rc<str>> {
    let input = varn_opt::OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names,
    };
    varn_opt::compile(input)
        .map_err(|e| -> Rc<str> { Rc::from(format!("varn-opt could not lower module: {e:?}")) })
}
