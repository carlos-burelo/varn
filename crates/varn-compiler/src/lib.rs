pub mod chunk;
pub mod codegen;
pub mod scope;

pub use chunk::{Chunk, FunctionProto, LineMapping, Literal, PoolEntry};
pub use codegen::Compiler;

use std::rc::Rc;
pub type GlobalLayout = FxHashMap<Rc<str>, usize>;

use rustc_hash::FxHashMap;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

pub fn compile(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
) -> Result<FunctionProto, Rc<str>> {
    codegen::compile_direct(
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
    )
}

pub fn compile_with_check_result(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
) -> Result<FunctionProto, Rc<str>> {
    codegen::compile_direct(
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
    )
}

pub fn compile_with_check_result_and_layout(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
    _global_layout: Option<GlobalLayout>,
) -> Result<FunctionProto, Rc<str>> {
    codegen::compile_direct(
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
    )
}
