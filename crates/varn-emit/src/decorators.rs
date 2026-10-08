use super::body::FnEmitter;
use super::functions::lower_outer;
use super::module_ctx::MCtx;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::FunctionDecl;
use varn_core::AtomInterner;
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirStmt, TirUnOp,
};
use varn_tir::{Signature, TirFunction, TyTable};

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
    shadowed: &rustc_hash::FxHashSet<u32>,
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
    let mut applied = false;
    for d in f.decorators.iter().rev().filter(|d| {
        !varn_core::ast::decorators::is_active_builtin(
            top.ast_arena,
            interner,
            |off| shadowed.contains(&off),
            d,
        )
    }) {
        applied = true;
        let deco = top.lower_expression(d.expression);
        cur = apply_one_decorator(top, cur, deco);
        top_body.extend(top.take_pending());
    }
    if !applied {
        return;
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
pub(super) fn lower_decorator_exprs(
    decorators: &[varn_core::ast::Decorator],
    ast_arena: &varn_core::ast::AstArena,
    ctx: &MCtx,
    expr_table: &FxHashMap<varn_core::ast::AstId, varn_sem::output::TypeEntry>,
    types: &mut TyTable,
    signatures: &mut Vec<Signature>,
    out: &mut Vec<TirFunction>,
    prelude: &mut Vec<TirStmt>,
    class_id: Option<varn_tir::ClassId>,
) -> Vec<TirExpr> {
    decorators
        .iter()
        .filter(|d| {
            !varn_core::ast::decorators::is_active_builtin(
                ast_arena,
                ctx.interner,
                |off| ctx.shadowed.contains(&off),
                d,
            )
        })
        .map(|d| {
            let (pre, x) = lower_outer(
                d.expression,
                ast_arena,
                ctx,
                expr_table,
                types,
                signatures,
                out,
                out.len() as u32,
                class_id,
            );
            prelude.extend(pre);
            x
        })
        .collect()
}
