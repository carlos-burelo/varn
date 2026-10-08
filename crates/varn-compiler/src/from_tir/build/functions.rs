use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, SsaFunc, Terminator, VarId};
use std::sync::Arc;
use varn_tir::{
    BackendTy, Resolution, TirExpr, TirExprKind, TirFunction, TirModule, TirStmt, TirUnOp,
};

pub(crate) fn defaulted_param_mask(body: &[TirStmt], nparams: usize) -> Vec<bool> {
    let mut mask = vec![false; nparams];
    for stmt in body {
        if let TirStmt::If { cond, .. } = stmt {
            if let TirExprKind::Unary {
                op: TirUnOp::IsNull,
                operand,
            } = &cond.kind
            {
                if let TirExpr {
                    kind: TirExprKind::Var,
                    res: Resolution::Param(i),
                    ..
                } = operand.as_ref()
                {
                    if (*i as usize) < nparams {
                        mask[*i as usize] = true;
                    }
                }
            }
        }
    }
    mask
}

pub fn build_function(
    tir: &TirModule,
    func: &TirFunction,
    self_fn: Option<varn_tir::FnId>,
    lines: &[u32],
) -> Result<SsaFunc> {
    build_inner(tir, func, false, &[], self_fn, lines)
}

pub fn build_top_level(
    tir: &TirModule,
    export_slots: &[Arc<str>],
    lines: &[u32],
) -> Result<SsaFunc> {
    build_inner(tir, &tir.top_level, true, export_slots, None, lines)
}

fn build_inner(
    tir: &TirModule,
    func: &TirFunction,
    register_module_fns: bool,
    export_slots: &[Arc<str>],
    self_fn: Option<varn_tir::FnId>,
    lines: &[u32],
) -> Result<SsaFunc> {
    let mut pinned = super::super::pinning::captured_vars(func);
    pinned.extend(super::super::pinning::try_pinned_vars(func));
    let mut b = Builder::with_pinned(tir, pinned.clone(), lines);
    b.self_fn = self_fn;
    b.locals_bt = func.locals.clone();
    b.return_bt = Some(func.return_ty);
    b.next_synthetic = func.locals.len() as u32;
    let entry = b.current;

    let defaulted = defaulted_param_mask(&func.body, func.params.len());
    for (i, pty) in func.params.iter().enumerate() {
        let t = if defaulted[i] {
            HirType::Dynamic
        } else {
            b.ty(*pty)
        };
        let v = b.new_value(t);
        b.block_mut(entry).params.push(v);
        let v = match (*pty, defaulted[i]) {
            (BackendTy::BigInt | BackendTy::Decimal, false) => {
                b.widen_exact(v, BackendTy::Int, *pty)
            }
            _ => v,
        };
        b.write_var(VarId::Param(i as u32), entry, v);
    }

    if register_module_fns {
        b.build_imports();
        for (i, f) in tir.functions.iter().enumerate() {
            if !super::ops::is_free_fn_name(&f.name) {
                continue;
            }
            let fv = b.emit(
                InstKind::MakeClosure {
                    func: i as u32,
                    upvalues_src: vec![],
                },
                HirType::Ref,
            );
            let store = b.global_store(&f.name, fv);
            b.emit_effect(store);
        }
        b.build_exports(export_slots);
    }

    b.lower_block(&func.body)?;
    if register_module_fns && b.is_open() {
        b.build_exports(export_slots);
    }
    if b.is_open() {
        b.set_term(Terminator::Return(None));
    }

    Ok(SsaFunc {
        name: func.name.clone(),
        entry,
        blocks: b.blocks,
        values: b.values,
        is_async: func.is_async,
        is_generator: func.is_generator,
    })
}

pub fn build_module(tir: &TirModule, lines: &[u32]) -> Result<Vec<SsaFunc>> {
    let mut out = Vec::with_capacity(tir.functions.len() + 1);
    out.push(build_function(tir, &tir.top_level, None, lines)?);
    for (i, f) in tir.functions.iter().enumerate() {
        out.push(build_function(
            tir,
            f,
            Some(varn_tir::FnId(i as u32)),
            lines,
        )?);
    }
    Ok(out)
}
