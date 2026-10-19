use crate::module::{TirClassDef, TirFunction};
use crate::node::{TirArg, TirArrayEl, TirExpr, TirExprKind, TirObjectEntry, TirStmt};

pub fn function_exprs_mut(func: &mut TirFunction, f: &mut impl FnMut(&mut TirExpr)) {
    stmts_mut(&mut func.body, f);
}

pub fn class_def_exprs_mut(def: &mut TirClassDef, f: &mut impl FnMut(&mut TirExpr)) {
    stmts_mut(&mut def.prelude, f);
    if let Some(e) = def.super_class.as_mut() {
        expr_mut(e, f);
    }
    for (_, init) in &mut def.statics {
        if let Some(e) = init.as_mut() {
            expr_mut(e, f);
        }
    }
    for m in &mut def.methods {
        m.decorators.iter_mut().for_each(|e| expr_mut(e, f));
    }
    for a in &mut def.accessors {
        a.decorators.iter_mut().for_each(|e| expr_mut(e, f));
    }
    def.decorators.iter_mut().for_each(|e| expr_mut(e, f));
    for p in &mut def.property_decorators {
        p.decorators.iter_mut().for_each(|e| expr_mut(e, f));
    }
    for v in &mut def.variants {
        v.const_args.iter_mut().for_each(|e| expr_mut(e, f));
    }
}

pub fn stmts_mut(stmts: &mut [TirStmt], f: &mut impl FnMut(&mut TirExpr)) {
    for s in stmts {
        stmt_mut(s, f);
    }
}

fn stmt_mut(stmt: &mut TirStmt, f: &mut impl FnMut(&mut TirExpr)) {
    match stmt {
        TirStmt::Expr(e) | TirStmt::Throw(e) => expr_mut(e, f),
        TirStmt::Let { init, .. } => {
            if let Some(e) = init.as_mut() {
                expr_mut(e, f);
            }
        }
        TirStmt::Return(value) => {
            if let Some(e) = value.as_mut() {
                expr_mut(e, f);
            }
        }
        TirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_mut(cond, f);
            stmts_mut(then_body, f);
            stmts_mut(else_body, f);
        }
        TirStmt::Loop { cond, body } => {
            expr_mut(cond, f);
            stmts_mut(body, f);
        }
        TirStmt::Try {
            body, catch_body, ..
        } => {
            stmts_mut(body, f);
            stmts_mut(catch_body, f);
        }
        TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
    }
}

fn args_mut(args: &mut [TirArg], f: &mut impl FnMut(&mut TirExpr)) {
    for a in args {
        match a {
            TirArg::Expr(e) | TirArg::Spread(e) | TirArg::Named { value: e, .. } => expr_mut(e, f),
        }
    }
}

pub fn expr_mut(expr: &mut TirExpr, f: &mut impl FnMut(&mut TirExpr)) {
    f(expr);
    match &mut expr.kind {
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Var
        | TirExprKind::DecimalLit(_)
        | TirExprKind::BigIntLit(_)
        | TirExprKind::Closure { .. } => {}
        TirExprKind::Binary { lhs, rhs, .. } => {
            expr_mut(lhs, f);
            expr_mut(rhs, f);
        }
        TirExprKind::Unary { operand, .. }
        | TirExprKind::Cast { operand }
        | TirExprKind::Checked { operand }
        | TirExprKind::ObjectKeys { operand } => expr_mut(operand, f),
        TirExprKind::Field { object, .. } | TirExprKind::ObjectRest { object, .. } => {
            expr_mut(object, f)
        }
        TirExprKind::Index { object, index } => {
            expr_mut(object, f);
            expr_mut(index, f);
        }
        TirExprKind::Call { callee, args } => {
            expr_mut(callee, f);
            args_mut(args, f);
        }
        TirExprKind::MethodCall { recv, args, .. }
        | TirExprKind::ExtensionCall { recv, args, .. } => {
            expr_mut(recv, f);
            args_mut(args, f);
        }
        TirExprKind::Assign { target, value } => {
            expr_mut(target, f);
            expr_mut(value, f);
        }
        TirExprKind::ArrayLit(els) => {
            for el in els {
                match el {
                    TirArrayEl::Expr(e) | TirArrayEl::Spread(e) => expr_mut(e, f),
                    TirArrayEl::Hole => {}
                }
            }
        }
        TirExprKind::TupleLit(items) => items.iter_mut().for_each(|e| expr_mut(e, f)),
        TirExprKind::ObjectLit { entries } => {
            for entry in entries {
                match entry {
                    TirObjectEntry::Field { value: e, .. } | TirObjectEntry::Spread(e) => {
                        expr_mut(e, f)
                    }
                }
            }
        }
        TirExprKind::RecordLit { fields } => fields.iter_mut().for_each(|(_, e)| expr_mut(e, f)),
        TirExprKind::Await { future } => expr_mut(future, f),
        TirExprKind::Yield { value, .. } => {
            if let Some(e) = value.as_mut() {
                expr_mut(e, f);
            }
        }
        TirExprKind::Discriminant { value }
        | TirExprKind::VariantPayload { value, .. }
        | TirExprKind::TypeTest { value, .. } => expr_mut(value, f),
        TirExprKind::New { args, .. }
        | TirExprKind::MakeVariant { args }
        | TirExprKind::SuperCall { args }
        | TirExprKind::SuperMethodCall { args, .. } => args_mut(args, f),
        TirExprKind::Select {
            cond,
            then_val,
            else_val,
        } => {
            expr_mut(cond, f);
            expr_mut(then_val, f);
            expr_mut(else_val, f);
        }
        TirExprKind::Seq { stmts, value } => {
            stmts_mut(stmts, f);
            expr_mut(value, f);
        }
        TirExprKind::RangeLit { start, end, .. } => {
            expr_mut(start, f);
            expr_mut(end, f);
        }
    }
}
