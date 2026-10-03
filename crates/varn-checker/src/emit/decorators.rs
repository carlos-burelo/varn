use super::body::FnEmitter;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::FunctionDecl;
use varn_core::AtomInterner;
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirStmt, TirUnOp,
};

pub(super) fn apply_one_decorator(top: &mut FnEmitter, prev: TirExpr, deco: TirExpr) -> TirExpr {
    let dyno = || BackendTy::Dynamic(DynReason::NotYetSupported);
    let applied = TirExpr {
        kind: TirExprKind::Call {
            callee: Box::new(deco),
            args: vec![TirArg::Expr(prev.clone())],
        },
        ty: dyno(),
        res: Resolution::None,
        span: Span::EMPTY,
    };
    let tmp = top.hoist(applied);
    TirExpr {
        kind: TirExprKind::Select {
            cond: Box::new(TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::IsNull,
                    operand: Box::new(tmp.clone()),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span: Span::EMPTY,
            }),
            then_val: Box::new(prev),
            else_val: Box::new(tmp),
        },
        ty: dyno(),
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

pub(super) fn emit_fn_decorator_app(
    f: &FunctionDecl,
    global_slots: &FxHashMap<Arc<str>, u32>,
    interner: &AtomInterner,
    top: &mut FnEmitter,
    top_body: &mut Vec<TirStmt>,
) {
    if f.decorators.is_empty() {
        return;
    }
    let name = interner.resolve(f.id);
    let Some(&slot) = global_slots.get(name) else {
        return;
    };
    let dyno = || BackendTy::Dynamic(DynReason::NotYetSupported);
    let global_ref = || TirExpr {
        kind: TirExprKind::Var,
        ty: dyno(),
        res: Resolution::GlobalSlot(slot),
        span: Span::EMPTY,
    };
    let mut cur = global_ref();
    for d in f.decorators.iter().rev() {
        let deco = top.lower_expression(d.expression);
        cur = apply_one_decorator(top, cur, deco);
        top_body.extend(top.take_pending());
    }
    top_body.push(TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign {
            target: Box::new(global_ref()),
            value: Box::new(cur),
        },
        ty: BackendTy::Void,
        res: Resolution::None,
        span: Span::EMPTY,
    }));
}
