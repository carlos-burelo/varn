pub mod class;
pub mod expr;
pub mod function;
pub mod ir;
pub mod liveness;
pub mod regalloc_post;
pub mod stmt;

pub use compiler::Compiler;
mod compiler;

use rustc_hash::FxHashMap;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

use crate::chunk::FunctionProto;

use std::rc::Rc;

pub fn compile_direct(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
    export_names: Vec<Rc<str>>,
) -> Result<FunctionProto, Rc<str>> {
    use std::cell::RefCell;
    let protos = Rc::new(RefCell::new(Vec::new()));
    let mut c = Compiler::new_module(
        program.filename.clone(),
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        protos,
        export_names,
    );
    c.compile_program(program);
    Ok(c.finish_module())
}
