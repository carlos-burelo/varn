use varn_tir::TirStmt;

pub(super) fn splice_finally_before_exits(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, false)
}

pub(super) fn splice_finally_before_exits_and_throw(
    stmts: Vec<TirStmt>,
    fin: &[TirStmt],
) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, true)
}

fn splice_finally_impl(stmts: Vec<TirStmt>, fin: &[TirStmt], on_throw: bool) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(_) | TirStmt::Break | TirStmt::Continue => {
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
                then_body: splice_finally_impl(then_body, fin, on_throw),
                else_body: splice_finally_impl(else_body, fin, on_throw),
            }),
            TirStmt::Loop { cond, body } => {
                out.push(TirStmt::Loop {
                    cond,
                    body: splice_returns_only(body, fin),
                });
            }
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: splice_finally_impl(body, fin, false),
                catch_local,
                catch_body: splice_finally_impl(catch_body, fin, on_throw),
            }),
            other @ TirStmt::Expr(_) | other @ TirStmt::Let { .. } | other @ TirStmt::Throw(_) | other @ TirStmt::BuildClass(_) => out.push(other),
        }
    }
    out
}

fn splice_returns_only(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(_) => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: splice_returns_only(then_body, fin),
                else_body: splice_returns_only(else_body, fin),
            }),
            TirStmt::Loop { cond, body } => out.push(TirStmt::Loop {
                cond,
                body: splice_returns_only(body, fin),
            }),
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: splice_returns_only(body, fin),
                catch_local,
                catch_body: splice_returns_only(catch_body, fin),
            }),
            other @ TirStmt::Expr(_) | other @ TirStmt::Let { .. } | other @ TirStmt::Break | other @ TirStmt::Continue | other @ TirStmt::Throw(_) | other @ TirStmt::BuildClass(_) => out.push(other),
        }
    }
    out
}
