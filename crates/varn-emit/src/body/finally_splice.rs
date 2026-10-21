use varn_tir::{BackendTy, LocalId, Resolution, Span, TirExpr, TirExprKind, TirStmt};

pub(super) type FreshLocal<'a> = &'a mut dyn FnMut(BackendTy) -> LocalId;

pub(super) fn splice_finally_before_exits(
    stmts: Vec<TirStmt>,
    fin: &[TirStmt],
    fresh: FreshLocal,
) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, false, fresh)
}

pub(super) fn splice_finally_before_exits_and_throw(
    stmts: Vec<TirStmt>,
    fin: &[TirStmt],
    fresh: FreshLocal,
) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, true, fresh)
}

fn return_after_finally(
    value: Option<TirExpr>,
    fin: &[TirStmt],
    fresh: FreshLocal,
    out: &mut Vec<TirStmt>,
) {
    let Some(value) = value else {
        out.extend(fin.iter().cloned());
        out.push(TirStmt::Return(None));
        return;
    };
    let ty = value.ty;
    let span: Span = value.span;
    let held = fresh(ty);
    out.push(TirStmt::Let {
        local: held,
        ty,
        init: Some(value),
    });
    out.extend(fin.iter().cloned());
    out.push(TirStmt::Return(Some(TirExpr {
        kind: TirExprKind::Var,
        ty,
        res: Resolution::Local(held),
        span,
    })));
}

fn splice_finally_impl(
    stmts: Vec<TirStmt>,
    fin: &[TirStmt],
    on_throw: bool,
    fresh: FreshLocal,
) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(value) => return_after_finally(value, fin, fresh, &mut out),
            TirStmt::Break | TirStmt::Continue => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::Throw(_) if on_throw => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: splice_finally_impl(then_body, fin, on_throw, fresh),
                else_body: splice_finally_impl(else_body, fin, on_throw, fresh),
            }),
            TirStmt::Loop { cond, body } => {
                out.push(TirStmt::Loop {
                    cond,
                    body: splice_returns_only(body, fin, fresh),
                });
            }
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: splice_finally_impl(body, fin, false, fresh),
                catch_local,
                catch_body: splice_finally_impl(catch_body, fin, on_throw, fresh),
            }),
            other @ TirStmt::Expr(_)
            | other @ TirStmt::Let { .. }
            | other @ TirStmt::Throw(_)
            | other @ TirStmt::BuildClass(_) => out.push(other),
        }
    }
    out
}

fn splice_returns_only(stmts: Vec<TirStmt>, fin: &[TirStmt], fresh: FreshLocal) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(value) => return_after_finally(value, fin, fresh, &mut out),
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: splice_returns_only(then_body, fin, fresh),
                else_body: splice_returns_only(else_body, fin, fresh),
            }),
            TirStmt::Loop { cond, body } => out.push(TirStmt::Loop {
                cond,
                body: splice_returns_only(body, fin, fresh),
            }),
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: splice_returns_only(body, fin, fresh),
                catch_local,
                catch_body: splice_returns_only(catch_body, fin, fresh),
            }),
            other @ TirStmt::Expr(_)
            | other @ TirStmt::Let { .. }
            | other @ TirStmt::Break
            | other @ TirStmt::Continue
            | other @ TirStmt::Throw(_)
            | other @ TirStmt::BuildClass(_) => out.push(other),
        }
    }
    out
}
