// Backend (shared with the deleted legacy path): SSA-free register allocation
// and liveness over emitted `FunctionProto`s. `varn-opt` produces the protos;
// these compress registers and feed the JIT.
pub mod ir;
pub mod liveness;
pub mod regalloc_post;

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
    // `varn-opt` is the sole backend: AST -> HIR -> SSA -> bytecode. The legacy
    // direct codegen has been deleted (Hito A).
    let input = varn_opt::OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names,
    };
    let mut proto = varn_opt::compile(input)
        .map_err(|e| -> Rc<str> { Rc::from(format!("varn-opt could not lower module: {e:?}")) })?;
    // The backend post-passes (`regalloc_post`, `slot_kinds`) live in this crate,
    // so `varn-opt` cannot run them itself without a dep cycle. Run them over the
    // whole proto tree (top level + every nested function proto in the constant
    // pools); without this, registers are uncompressed and `register_meta` is
    // empty, disabling the JIT's typed fast paths.
    run_backend_post_passes(&mut proto);
    Ok(proto)
}

/// Apply the register-allocation post-pass and slot-kind inference to a proto
/// produced by `varn-opt`, recursing into every nested function proto embedded
/// in the constant pools.
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
