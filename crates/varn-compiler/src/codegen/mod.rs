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

    // Optimizer tier (temporary `VN_OPT` dev gate). When set, route through the
    // `varn-opt` pipeline (AST -> HIR -> SSA -> opt -> bytecode); any construct
    // it doesn't yet lower returns Err and we fall back to the legacy codegen
    // below, so the compiler stays functional while varn-opt is brought up.
    if std::env::var_os("VN_OPT").is_some() {
        let input = varn_opt::OptInput {
            program,
            annotations,
            extension_calls,
            extension_members,
            extension_set_members,
            export_names: export_names.clone(),
        };
        if let Ok(proto) = varn_opt::compile(input) {
            return Ok(proto);
        }
    }

    let escape_analysis = Rc::new(crate::analysis::escape::EscapeAnalysis::analyze(program));
    let inline_registry = Rc::new(crate::analysis::inline::InlineRegistry::analyze(program));
    let protos = Rc::new(RefCell::new(Vec::new()));
    let mut c = Compiler::new_module(
        program.filename.clone(),
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        protos,
        export_names,
        escape_analysis,
        inline_registry,
    );
    c.compile_program(program);
    if let Some(err) = c.error {
        return Err(err);
    }
    Ok(c.finish_module())
}
