use std::sync::Arc;

use varn_tir::{BackendTy, TirFunction, TirModule};
use varn_types::chunk::FunctionProto;

use crate::hir::{HirType, TyTable as SsaTyTable};
use crate::ssa::emit::{emit_function_meta, slot_kind_of, FnMeta};
use crate::OptError;

use super::build::{build_function, build_top_level};
use super::ty::lower as lower_ty;

type Result<T> = std::result::Result<T, OptError>;

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

pub(crate) fn emit_tir_closure(idx: u32, source_file: Arc<str>) -> Result<FunctionProto> {
    let ptr = CUR_TIR.with(|c| c.get());
    assert!(
        !ptr.is_null(),
        "from_tir: closure emitted outside a module scope"
    );

    let tir: &TirModule = unsafe { &*ptr };
    compile_closure(tir, idx, source_file)
}

fn fn_meta(tir: &TirModule, f: &TirFunction) -> FnMeta {
    let mut tt = SsaTyTable::default();
    let mut ty = |bt: BackendTy| -> HirType { lower_ty(bt, tir, &mut tt) };

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
    source_file: Arc<str>,
    export_slots: &[Arc<str>],
    self_fn: Option<varn_tir::FnId>,
) -> Result<FunctionProto> {
    let mut ssa = if is_top_level {
        build_top_level(tir, export_slots)?
    } else {
        build_function(tir, f, self_fn)?
    };
    crate::ssa::verify::recompute_preds(&mut ssa);
    crate::passes::optimize(&mut ssa);
    let state_size = crate::passes::state_machine::run(&mut ssa);
    crate::ssa::verify::recompute_preds(&mut ssa);
    if let Err(why) = crate::ssa::verify::verify(&ssa) {
        panic!("from_tir: ssa verify failed for {}: {}", f.name, why);
    }
    let mut proto = emit_function_meta(ssa, &fn_meta(tir, f), source_file)?;
    proto.state_size = state_size;
    if is_top_level {
        proto.global_count = tir.global_names.len() as u32;
    }
    Ok(proto)
}

pub(crate) fn compile_closure(
    tir: &TirModule,
    idx: u32,
    source_file: Arc<str>,
) -> Result<FunctionProto> {
    let f = tir
        .functions
        .get(idx as usize)
        .ok_or(OptError::Unsupported(
            "from_tir: closure index out of range",
        ))?;
    compile_one(tir, f, false, source_file, &[], Some(varn_tir::FnId(idx)))
}

pub fn compile_module(tir: &TirModule, export_names: Vec<Arc<str>>) -> Result<FunctionProto> {
    varn_tir::verify_module(tir).map_err(|errors| {
        crate::OptError::InvalidTir(
            errors
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}..{}: {}",
                        tir.source_file, e.span.start, e.span.end, e.message
                    )
                })
                .collect(),
        )
    })?;
    let _scope = enter_module(tir);
    let source_file = tir.source_file.clone();
    let mut proto = compile_one(tir, &tir.top_level, true, source_file, &export_names, None)?;
    proto.export_names = export_names
        .into_iter()
        .map(|s| Arc::from(s.as_ref()))
        .collect();

    crate::regalloc::run_post_passes(&mut proto);
    Ok(proto)
}
