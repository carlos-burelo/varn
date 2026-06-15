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
        match varn_opt::compile(input) {
            Ok(mut proto) => {
                // §1.0: the backend post-passes (`regalloc_post`, `slot_kinds`)
                // live in this crate, so `varn-opt` cannot run them itself
                // without a dep cycle. Run them here over the whole proto tree
                // (top level + every nested function proto in the constant
                // pools), exactly as `finish_module`/`finish_function` do for
                // the legacy path. Without this, varn-opt code skips register
                // compression and leaves `register_meta` empty, disabling the
                // JIT's typed fast paths.
                run_backend_post_passes(&mut proto);
                return Ok(proto);
            }
            Err(e) => {
                if std::env::var_os("VN_OPT_TRACE").is_some() {
                    eprintln!(
                        "[varn-opt] fallback ({}): {:?}",
                        program.filename, e
                    );
                }
            }
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

/// Apply the register-allocation post-pass and slot-kind inference to a proto
/// produced by `varn-opt`, recursing into every nested function proto embedded
/// in the constant pools. Mirrors `Compiler::finish_module`/`finish_function`,
/// which run these two passes per emitted proto in the legacy path.
///
/// Nested protos sit in the constant pool behind `Rc`. They are freshly built
/// (refcount 1) so `Rc::get_mut` succeeds; the clone-and-replace branch is a
/// defensive fallback should a proto ever be shared.
fn run_backend_post_passes(proto: &mut FunctionProto) {
    use varn_types::chunk::PoolEntry;

    for entry in proto.chunk.constants.iter_mut() {
        if let PoolEntry::Function(rc) = entry {
            match Rc::get_mut(rc) {
                Some(inner) => run_backend_post_passes(inner),
                None => {
                    let mut cloned = (**rc).clone();
                    run_backend_post_passes(&mut cloned);
                    *rc = Rc::new(cloned);
                }
            }
        }
    }

    regalloc_post::optimize_function(proto);
    crate::analysis::slot_kinds::infer(proto);
}
