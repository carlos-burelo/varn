//! `TirModule` -> `FunctionProto` (step 3.4 wiring).
//!
//! Runs each `SsaFunc` from `build_module` through the same optimisation,
//! state-machine and emission pipeline the HIR path uses. Closures reference
//! their body by TIR index (`ClosureBody::Tir`); `ssa/emit` calls back here
//! to compile them.

use std::rc::Rc;

use varn_tir::{BackendTy, TirFunction, TirModule};
use varn_types::chunk::FunctionProto;

use crate::hir::{HirType, TyTable as SsaTyTable};
use crate::ssa::emit::{emit_function_meta, slot_kind_of, FnMeta};
use crate::OptError;

use super::build::{build_function, build_top_level};
use super::ty::lower as lower_ty;

type Result<T> = std::result::Result<T, OptError>;

// The `TirModule` currently being compiled, so `ssa/emit`'s `ClosureBody::Tir`
// arm can call back to compile the referenced body. Same raw-pointer scope
// guard pattern as `hir::ctor_summary::Scope` / the VM's `clif_link::CtxGuard`
// — the pointer is only live for the duration of `with_module`, which owns
// the borrow.
thread_local! {
    static CUR_TIR: std::cell::Cell<*const TirModule> = const { std::cell::Cell::new(std::ptr::null()) };
}

struct ModuleScope(*const TirModule);
impl Drop for ModuleScope {
    fn drop(&mut self) {
        CUR_TIR.with(|c| c.set(self.0));
    }
}
fn enter_module(tir: &TirModule) -> ModuleScope {
    let prev = CUR_TIR.with(|c| c.replace(tir as *const TirModule));
    ModuleScope(prev)
}

/// Compile the closure body a `ClosureBody::Tir(idx)` names, using the module
/// set by the enclosing `enter_module`. Panics if called outside one — that
/// only happens if a `from_tir` SSA function reached emission without going
/// through `compile_module` / `compile_closure`.
pub(crate) fn emit_tir_closure(idx: u32, source_file: Rc<str>) -> Result<FunctionProto> {
    let ptr = CUR_TIR.with(|c| c.get());
    assert!(
        !ptr.is_null(),
        "from_tir: closure emitted outside a module scope"
    );
    // SAFETY: `ptr` was set by `enter_module` from a live `&TirModule` whose
    // borrow outlives this call (it is on the stack of `compile_module`).
    let tir: &TirModule = unsafe { &*ptr };
    compile_closure(tir, idx, source_file)
}

fn fn_meta(tir: &TirModule, f: &TirFunction) -> FnMeta {
    let mut tt = SsaTyTable::default();
    let mut ty = |bt: BackendTy| -> HirType { lower_ty(bt, tir, &mut tt) };
    // A defaulted param's home register is `Dynamic` (see
    // `build::defaulted_param_mask` / `build_inner`): it must arrive able to
    // hold `null` before the prologue applies the default. `derive_register_meta`
    // would converge on `Dynamic` for it anyway once it meets that entry
    // value's kind against this one, but declaring it here too keeps the
    // ABI-facing `param_kinds` honest instead of relying on the meet to paper
    // over a kind this function already knows is wrong.
    let defaulted = super::build::defaulted_param_mask(&f.body, f.params.len());
    FnMeta {
        name: f.name.clone(),
        start_line: 1,
        nparams: f.params.len(),
        param_kinds: f
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if defaulted[i] {
                    varn_types::register_meta::SlotKind::Dynamic
                } else {
                    slot_kind_of(ty(*p))
                }
            })
            .collect(),
        return_kind: slot_kind_of(ty(f.return_ty)),
        has_rest: f.has_rest,
        is_async: f.is_async,
        is_generator: f.is_generator,
        has_this: f.has_this,
        upvalue_count: 0,
    }
}

fn compile_one(
    tir: &TirModule,
    f: &TirFunction,
    is_top_level: bool,
    source_file: Rc<str>,
    export_slots: &[Rc<str>],
    self_fn: Option<varn_tir::FnId>,
) -> Result<FunctionProto> {
    let mut ssa = if is_top_level {
        build_top_level(tir, export_slots)?
    } else {
        build_function(tir, f, self_fn)?
    };
    crate::ssa::verify::recompute_preds(&mut ssa);
    crate::passes::optimize_with(&mut ssa, &super::ctor_summary::current());
    let state_size = crate::passes::state_machine::run(&mut ssa);
    crate::ssa::verify::recompute_preds(&mut ssa);
    if let Err(why) = crate::ssa::verify::verify(&ssa) {
        panic!("from_tir: ssa verify failed for {}: {}", f.name, why);
    }
    let mut proto = emit_function_meta(ssa, &fn_meta(tir, f), source_file)?;
    proto.state_size = state_size;
    if is_top_level {
        // The module owns this many global slots; the VM reserves a contiguous
        // region for them and the indexed global opcodes are region-relative.
        proto.global_count = tir.global_names.len() as u32;
    }
    Ok(proto)
}

/// Compile the body a `ClosureBody::Tir(idx)` refers to. Called from
/// `ssa/emit` while emitting a `MakeClosure`.
pub(crate) fn compile_closure(
    tir: &TirModule,
    idx: u32,
    source_file: Rc<str>,
) -> Result<FunctionProto> {
    let f = tir
        .functions
        .get(idx as usize)
        .ok_or(OptError::Unsupported(
            "from_tir: closure index out of range",
        ))?;
    compile_one(tir, f, false, source_file, &[], Some(varn_tir::FnId(idx)))
}

/// Compile a whole module: the top-level proto, with every free function and
/// method stored as a global by name (the HIR path's convention).
pub fn compile_module(tir: &TirModule, export_names: Vec<Rc<str>>) -> Result<FunctionProto> {
    let _scope = enter_module(tir);
    let summaries = super::ctor_summary::collect(tir);
    let _ctor_scope = super::ctor_summary::Scope::enter(summaries);
    let source_file = tir.source_file.clone();
    let mut proto = compile_one(tir, &tir.top_level, true, source_file, &export_names, None)?;
    proto.export_names = export_names;
    // Coalescing + register-count validation, recursing into nested protos.
    crate::regalloc::run_post_passes(&mut proto);
    Ok(proto)
}
