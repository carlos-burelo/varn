use varn_tir::{TirExpr, TirExprKind, TirStmt};

fn arg_exprs(args: &[varn_tir::TirArg]) -> impl Iterator<Item = &TirExpr> {
    args.iter().map(|a| match a {
        varn_tir::TirArg::Expr(x)
        | varn_tir::TirArg::Named { value: x, .. }
        | varn_tir::TirArg::Spread(x) => x,
    })
}

pub(crate) fn child_exprs(e: &TirExpr) -> Vec<&TirExpr> {
    use TirExprKind::*;
    let mut out: Vec<&TirExpr> = Vec::new();
    match &e.kind {
        IntLit(_)
        | FloatLit(_)
        | BoolLit(_)
        | StrLit(_)
        | CharLit(_)
        | NullLit
        | Var
        | Closure { .. }
        | DecimalLit(_)
        | BigIntLit(_) => {}
        RangeLit { start, end, .. } => {
            out.push(start);
            out.push(end);
        }
        ObjectRest { object, .. } => out.push(object),
        ExtensionCall { recv, args, .. } => {
            out.push(recv);
            out.extend(arg_exprs(args));
        }
        Binary { lhs, rhs, .. } => {
            out.push(lhs);
            out.push(rhs);
        }
        Unary { operand, .. } | Cast { operand } | ObjectKeys { operand } => out.push(operand),
        IterInit { source, .. } => out.push(source),
        Field { object, .. } => out.push(object),
        Index { object, index } => {
            out.push(object);
            out.push(index);
        }
        Call { callee, args } => {
            out.push(callee);
            out.extend(arg_exprs(args));
        }
        MethodCall { recv, args, .. } => {
            out.push(recv);
            out.extend(arg_exprs(args));
        }
        Assign { target, value } => {
            out.push(target);
            out.push(value);
        }
        ArrayLit(els) => {
            for el in els {
                match el {
                    varn_tir::TirArrayEl::Expr(x) | varn_tir::TirArrayEl::Spread(x) => out.push(x),
                    varn_tir::TirArrayEl::Hole => {}
                }
            }
        }
        TupleLit(xs) => out.extend(xs.iter()),
        RecordLit { fields } => out.extend(fields.iter().map(|(_, v)| v)),
        ObjectLit { entries } => {
            for en in entries {
                match en {
                    varn_tir::TirObjectEntry::Field { value, .. } => out.push(value),
                    varn_tir::TirObjectEntry::Spread(x) => out.push(x),
                }
            }
        }
        Await { future } => out.push(future),
        Yield { value, .. } => out.extend(value.as_deref()),
        Discriminant { value } | VariantPayload { value, .. } | TypeTest { value, .. } => {
            out.push(value)
        }
        New { args, .. }
        | MakeVariant { args }
        | SuperCall { args }
        | SuperMethodCall { args, .. } => out.extend(arg_exprs(args)),
        Select {
            cond,
            then_val,
            else_val,
        } => {
            out.push(cond);
            out.push(then_val);
            out.push(else_val);
        }
        Seq { value, .. } => out.push(value),
    }
    out
}

fn stmt_exprs(s: &TirStmt) -> Vec<&TirExpr> {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => vec![e],
        TirStmt::Let { init, .. } => init.iter().collect(),
        TirStmt::Return(v) => v.iter().collect(),
        TirStmt::If { cond, .. } | TirStmt::Loop { cond, .. } => vec![cond],
        TirStmt::Break | TirStmt::Continue | TirStmt::Try { .. } | TirStmt::BuildClass(_) => {
            Vec::new()
        }
    }
}

pub(crate) fn seq_bodies(s: &TirStmt) -> Vec<&[TirStmt]> {
    let mut out = Vec::new();
    let mut stack = stmt_exprs(s);
    while let Some(e) = stack.pop() {
        if let TirExprKind::Seq { stmts, .. } = &e.kind {
            out.push(stmts.as_slice());
        }
        stack.extend(child_exprs(e));
    }
    out
}
