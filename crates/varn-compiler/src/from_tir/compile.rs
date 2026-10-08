use std::sync::Arc;
use std::time::Duration;

use varn_tir::{BackendTy, TirFunction, TirModule};
use varn_types::chunk::FunctionProto;

use crate::hir::{HirType, TyTable as SsaTyTable};
use crate::ssa::emit::{emit_function_meta, slot_kind_of, FnMeta};
use crate::OptError;

use super::build::{build_function, build_top_level};
use super::ty::lower as lower_ty;

type Result<T> = std::result::Result<T, OptError>;

pub(crate) struct ModuleScope<'m> {
    pub tir: &'m TirModule,
    pub lines: &'m [u32],
}

pub fn line_starts_of(source: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i as u32 + 1);
        }
    }
    starts
}

pub(crate) fn compile_closure(
    scope: &ModuleScope,
    idx: u32,
    source_file: Arc<str>,
) -> Result<FunctionProto> {
    let tir = scope.tir;
    let f = tir
        .functions
        .get(idx as usize)
        .ok_or(OptError::Unsupported(
            "from_tir: closure index out of range",
        ))?;
    compile_one(scope, f, false, source_file, &[], Some(varn_tir::FnId(idx)))
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
    scope: &ModuleScope,
    f: &TirFunction,
    is_top_level: bool,
    source_file: Arc<str>,
    export_slots: &[Arc<str>],
    self_fn: Option<varn_tir::FnId>,
) -> Result<FunctionProto> {
    let tir = scope.tir;
    let mut ssa = if is_top_level {
        build_top_level(tir, export_slots, scope.lines)?
    } else {
        build_function(tir, f, self_fn, scope.lines)?
    };
    crate::ssa::verify::recompute_preds(&mut ssa);
    crate::passes::optimize(&mut ssa);
    let state_size = crate::passes::state_machine::run(&mut ssa);
    crate::ssa::verify::recompute_preds(&mut ssa);
    if let Err(why) = crate::ssa::verify::verify(&ssa) {
        return Err(crate::OptError::Internal(format!(
            "from_tir: ssa verify failed for {}: {}",
            f.name, why
        )));
    }
    let mut proto = emit_function_meta(ssa, &fn_meta(tir, f), source_file, scope)?;
    proto.state_size = state_size;
    if is_top_level {
        proto.global_count = tir.global_names.len() as u32;
    }
    Ok(proto)
}

pub fn compile_module(
    tir: &TirModule,
    export_names: Vec<Arc<str>>,
    source: &str,
    measure: bool,
) -> (std::result::Result<FunctionProto, OptError>, Duration) {
    if let Err(errors) = varn_tir::verify_module(tir) {
        return (
            Err(crate::OptError::InvalidTir(
                errors
                    .iter()
                    .map(|e| {
                        format!(
                            "{}:{}..{}: {}",
                            tir.source_file, e.span.start, e.span.end, e.message
                        )
                    })
                    .collect(),
            )),
            Duration::ZERO,
        );
    }
    let lines = line_starts_of(source);
    let scope = ModuleScope { tir, lines: &lines };
    let source_file = tir.source_file.clone();
    let mut proto = match compile_one(
        &scope,
        &tir.top_level,
        true,
        source_file,
        &export_names,
        None,
    ) {
        Ok(p) => p,
        Err(e) => return (Err(e), Duration::ZERO),
    };
    proto.export_names = export_names
        .into_iter()
        .map(|s| Arc::from(s.as_ref()))
        .collect();

    let opt_time = crate::regalloc::run_post_passes(&mut proto, measure);
    (Ok(proto), opt_time)
}
