use super::bindings::{check_condition, check_let, check_return, check_return_none};
use super::calls::{check_direct_call, check_method_call};
use super::ops::{check_binary, check_field, check_index, check_variant_payload};
use crate::node::{
    TirArg, TirArrayEl, TirExpr, TirExprKind, TirFunction, TirModule, TirObjectEntry, TirStmt,
    TirUnOp,
};
use crate::ty::BackendTy;
use crate::verify::VerifyError;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    for s in &f.body {
        walk_stmt(m, f, s, errors);
    }
}

fn walk_stmt(m: &TirModule, f: &TirFunction, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(m, f, e, errors),
        TirStmt::Let { ty, init, .. } => {
            if let Some(e) = init {
                walk_expr(m, f, e, errors);
                check_let(m, *ty, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                walk_expr(m, f, e, errors);
                check_return(m, f, e, errors);
            } else {
                // Bare Return(None) requires function to return Void
                check_return_none(m, f, errors);
            }
        }
        TirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            walk_expr(m, f, cond, errors);
            check_condition(m, "if", cond, errors);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            walk_expr(m, f, cond, errors);
            check_condition(m, "loop", cond, errors);
            for s in body {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Try {
            body, catch_body, ..
        } => {
            for s in body.iter().chain(catch_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
    }
}

fn walk_arg(m: &TirModule, f: &TirFunction, a: &TirArg, errors: &mut Vec<VerifyError>) {
    walk_expr(m, f, a.value(), errors);
}

fn walk_expr(m: &TirModule, f: &TirFunction, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    match &e.kind {
        TirExprKind::Binary { op, lhs, rhs } => {
            walk_expr(m, f, lhs, errors);
            walk_expr(m, f, rhs, errors);
            check_binary(m, e, *op, lhs, rhs, errors);
        }
        TirExprKind::Field { object, .. } => {
            walk_expr(m, f, object, errors);
            check_field(m, e, object, errors);
        }
        TirExprKind::Index { object, index } => {
            walk_expr(m, f, object, errors);
            walk_expr(m, f, index, errors);
            check_index(m, e, object, errors);
        }
        TirExprKind::Unary { op, operand } => {
            walk_expr(m, f, operand, errors);
            if *op == TirUnOp::IsNull && e.ty != BackendTy::Bool {
                errors.push(VerifyError::new(
                    format!("IsNull must produce Bool, node says {:?}", e.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::Cast { operand } => walk_expr(m, f, operand, errors),
        TirExprKind::Call { callee, args } => {
            walk_expr(m, f, callee, errors);
            for a in args {
                walk_arg(m, f, a, errors);
            }
            check_direct_call(m, e, args, errors);
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            walk_expr(m, f, recv, errors);
            for a in args {
                walk_arg(m, f, a, errors);
            }
            check_method_call(m, e, recv, args, errors);
        }
        TirExprKind::Assign { target, value } => {
            walk_expr(m, f, target, errors);
            walk_expr(m, f, value, errors);
        }
        TirExprKind::TupleLit(xs) => {
            for x in xs {
                walk_expr(m, f, x, errors);
            }
        }
        TirExprKind::RecordLit { fields } => {
            for (_, v) in fields {
                walk_expr(m, f, v, errors);
            }
        }
        TirExprKind::ArrayLit(els) => {
            for el in els {
                match el {
                    TirArrayEl::Expr(x) | TirArrayEl::Spread(x) => walk_expr(m, f, x, errors),
                    TirArrayEl::Hole => {}
                }
            }
        }
        TirExprKind::ObjectLit { entries } => {
            for entry in entries {
                match entry {
                    TirObjectEntry::Field { value, .. } | TirObjectEntry::Spread(value) => {
                        walk_expr(m, f, value, errors)
                    }
                }
            }
        }
        TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
            for a in args {
                walk_arg(m, f, a, errors);
            }
        }
        TirExprKind::Await { future } => {
            walk_expr(m, f, future, errors);
            if !f.is_async {
                errors.push(VerifyError::new(
                    format!("`await` in `{}`, which is not async", f.name),
                    e.span,
                ));
            }
        }
        TirExprKind::Yield { value, .. } => {
            if let Some(v) = value {
                walk_expr(m, f, v, errors);
            }
            if !f.is_generator {
                errors.push(VerifyError::new(
                    format!("`yield` in `{}`, which is not a generator", f.name),
                    e.span,
                ));
            }
        }
        TirExprKind::Discriminant { value } => {
            walk_expr(m, f, value, errors);
            if e.ty != BackendTy::Int {
                errors.push(VerifyError::new(
                    format!("Discriminant must produce Int, node says {:?}", e.ty),
                    e.span,
                ));
            }
            if !matches!(
                value.ty.non_nullable(&m.types),
                BackendTy::Enum(_) | BackendTy::Dynamic(_)
            ) {
                errors.push(VerifyError::new(
                    format!("Discriminant of {:?}, which is not an enum", value.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::VariantPayload { value, tag, field } => {
            walk_expr(m, f, value, errors);
            check_variant_payload(m, e, value, *tag, *field, errors);
        }
        TirExprKind::TypeTest { value, .. } => {
            walk_expr(m, f, value, errors);
            if e.ty != BackendTy::Bool {
                errors.push(VerifyError::new(
                    format!("TypeTest must produce Bool, node says {:?}", e.ty),
                    e.span,
                ));
            }
        }
        TirExprKind::ObjectKeys { operand } => walk_expr(m, f, operand, errors),
        TirExprKind::IterInit { source, .. } => walk_expr(m, f, source, errors),
        TirExprKind::SuperCall { args } | TirExprKind::SuperMethodCall { args, .. } => {
            for a in args {
                walk_arg(m, f, a, errors);
            }
        }
        TirExprKind::RangeLit { start, end, .. } => {
            walk_expr(m, f, start, errors);
            walk_expr(m, f, end, errors);
        }
        TirExprKind::DecimalLit(_) | TirExprKind::BigIntLit(_) => {}
        TirExprKind::ObjectRest { object, .. } => walk_expr(m, f, object, errors),
        TirExprKind::ExtensionCall { recv, args, .. } => {
            walk_expr(m, f, recv, errors);
            for a in args {
                walk_arg(m, f, a, errors);
            }
        }
        TirExprKind::Select {
            cond,
            then_val,
            else_val,
        } => {
            walk_expr(m, f, cond, errors);
            walk_expr(m, f, then_val, errors);
            walk_expr(m, f, else_val, errors);
            if then_val.ty != else_val.ty && e.ty != BackendTy::Dynamic(crate::ty::DynReason::Union)
            {
                errors.push(VerifyError::new(
                    "Select arms have different types and the result is not a union",
                    e.span,
                ));
            }
        }
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Closure { .. }
        | TirExprKind::Var => {}
    }
}
