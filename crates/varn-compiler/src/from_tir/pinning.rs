use rustc_hash::FxHashSet;
use varn_tir::{Resolution, TirExpr, TirExprKind, TirStmt};

use super::tir_children::{child_exprs, seq_bodies};
use crate::ssa::ir::VarId;

pub(super) fn captured_vars(func: &varn_tir::TirFunction) -> FxHashSet<VarId> {
    use crate::hir::LocalId;
    use varn_tir::TirUpvalue;
    let mut out = FxHashSet::default();
    fn walk_expr(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Closure { upvalues, .. } = &e.kind {
            for u in upvalues {
                match u {
                    TirUpvalue::ParentLocal(i) => {
                        out.insert(VarId::Local(LocalId(*i)));
                    }
                    TirUpvalue::ParentParam(i) => {
                        out.insert(VarId::Param(*i));
                    }
                    TirUpvalue::ParentUpvalue(_) => {}
                }
            }
        }
        if let TirExprKind::Seq { stmts, .. } = &e.kind {
            walk_body(stmts, out);
        }
        for c in child_exprs(e) {
            walk_expr(c, out);
        }
    }
    fn walk_body(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            match stmt {
                TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(e, out),
                TirStmt::Let { init: Some(e), .. } | TirStmt::Return(Some(e)) => walk_expr(e, out),
                TirStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    walk_expr(cond, out);
                    walk_body(then_body, out);
                    walk_body(else_body, out);
                }
                TirStmt::Loop { cond, body } => {
                    walk_expr(cond, out);
                    walk_body(body, out);
                }
                TirStmt::Try {
                    body, catch_body, ..
                } => {
                    walk_body(body, out);
                    walk_body(catch_body, out);
                }
                TirStmt::Let { .. }
                | TirStmt::Return(_)
                | TirStmt::Break
                | TirStmt::Continue
                | TirStmt::BuildClass(_) => {}
            }
        }
    }
    walk_body(&func.body, &mut out);
    out
}

pub(super) fn try_pinned_vars(func: &varn_tir::TirFunction) -> FxHashSet<VarId> {
    let mut out = FxHashSet::default();
    fn note_target(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Var = &e.kind {
            match &e.res {
                Resolution::Local(id) => {
                    out.insert(VarId::Local(crate::hir::LocalId(id.0)));
                }
                Resolution::Param(i) => {
                    out.insert(VarId::Param(*i));
                }
                Resolution::None
                | Resolution::Upvalue(_)
                | Resolution::GlobalSlot(_)
                | Resolution::NativeGlobal(_)
                | Resolution::ModuleSlot { .. }
                | Resolution::FieldSlot(_)
                | Resolution::StaticField(_)
                | Resolution::VtableSlot(_)
                | Resolution::DirectFn(_)
                | Resolution::Intrinsic(_)
                | Resolution::NativeOp(_)
                | Resolution::EnumVariant { .. }
                | Resolution::ByName { .. } => {}
            }
        }
    }
    fn scan_assigns(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            match stmt {
                TirStmt::Expr(e) | TirStmt::Throw(e) | TirStmt::Return(Some(e)) => {
                    scan_expr_assigns(e, out)
                }
                TirStmt::Let { local, .. } => {
                    out.insert(VarId::Local(crate::hir::LocalId(local.0)));
                }
                TirStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scan_assigns(then_body, out);
                    scan_assigns(else_body, out);
                }
                TirStmt::Loop { body, .. } => scan_assigns(body, out),
                TirStmt::Try {
                    body, catch_body, ..
                } => {
                    scan_assigns(body, out);
                    scan_assigns(catch_body, out);
                }
                TirStmt::Return(_)
                | TirStmt::Break
                | TirStmt::Continue
                | TirStmt::BuildClass(_) => {}
            }
        }
    }
    fn scan_expr_assigns(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Assign { target, value } = &e.kind {
            note_target(target, out);
            scan_expr_assigns(value, out);
        }
        if let TirExprKind::Seq { stmts, .. } = &e.kind {
            scan_assigns(stmts, out);
        }
        for c in child_exprs(e) {
            scan_expr_assigns(c, out);
        }
    }
    fn walk(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            for nested in seq_bodies(stmt) {
                walk(nested, out);
            }
            match stmt {
                TirStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, out);
                    walk(else_body, out);
                }
                TirStmt::Loop { body, .. } => walk(body, out),
                TirStmt::Try {
                    body, catch_body, ..
                } => {
                    scan_assigns(body, out);
                    scan_assigns(catch_body, out);

                    walk(body, out);
                    walk(catch_body, out);
                }
                TirStmt::Expr(_)
                | TirStmt::Let { .. }
                | TirStmt::Return(_)
                | TirStmt::Break
                | TirStmt::Continue
                | TirStmt::Throw(_)
                | TirStmt::BuildClass(_) => {}
            }
        }
    }
    walk(&func.body, &mut out);
    out
}
