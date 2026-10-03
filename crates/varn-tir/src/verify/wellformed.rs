//! Every handle points at something that exists; every slot is in range.

mod handles;

use super::VerifyError;
use crate::node::{TirExpr, TirExprKind, TirStmt};
use crate::resolution::Resolution;
use crate::ty::BackendTy;
use crate::{TirFunction, TirModule};
use handles::{check_res, check_ty};

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_declarations(m, errors);
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_declarations(m: &TirModule, errors: &mut Vec<VerifyError>) {
    // Check globals
    let dummy_expr = TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Void,
        res: Resolution::None,
        span: crate::node::Span::EMPTY,
    };
    for ty in &m.globals {
        check_ty(m, *ty, &dummy_expr, errors);
    }

    // Check class fields
    for class in &m.classes {
        for field in &class.fields {
            check_ty(m, field.ty, &dummy_expr, errors);
        }
        // Every vtable entry must name a real signature — a dangling SigId
        // here silently disables check_method_call's arity/type coherence
        // rule (it looks up the signature and just returns if absent).
        for entry in &class.vtable {
            if m.signature(entry.sig).is_none() {
                errors.push(VerifyError::new(
                    format!(
                        "vtable entry `{}` names SigId({}), which has no entry",
                        entry.name, entry.sig.0
                    ),
                    dummy_expr.span,
                ));
            }
        }
    }

    // Check enum variants
    for enum_info in &m.enums {
        for variant in &enum_info.variants {
            for ty in &variant.payload {
                check_ty(m, *ty, &dummy_expr, errors);
            }
        }
    }

    // Check signatures
    for sig in &m.signatures {
        for param_ty in &sig.params {
            check_ty(m, *param_ty, &dummy_expr, errors);
        }
        check_ty(m, sig.return_ty, &dummy_expr, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    if m.signature(f.sig).is_none() {
        errors.push(VerifyError::new(
            format!(
                "function `{}` names SigId({}), which has no entry",
                f.name, f.sig.0
            ),
            crate::node::Span::EMPTY,
        ));
    }

    // Check function's own type declarations
    let dummy_expr = TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Void,
        res: Resolution::None,
        span: crate::node::Span::EMPTY,
    };
    for param_ty in &f.params {
        check_ty(m, *param_ty, &dummy_expr, errors);
    }
    check_ty(m, f.return_ty, &dummy_expr, errors);
    for local_ty in &f.locals {
        check_ty(m, *local_ty, &dummy_expr, errors);
    }

    for s in &f.body {
        check_stmt(m, f, s, errors);
    }
}

fn check_stmt(m: &TirModule, f: &TirFunction, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => check_expr(m, f, e, errors),
        TirStmt::Let {
            ty, init, local, ..
        } => {
            let dummy_expr = TirExpr {
                kind: TirExprKind::NullLit,
                ty: BackendTy::Void,
                res: Resolution::None,
                span: crate::node::Span::EMPTY,
            };
            check_ty(m, *ty, &dummy_expr, errors);
            if local.0 as usize >= f.locals.len() {
                errors.push(VerifyError::new(
                    format!("Let binds LocalId({}), which is out of range", local.0),
                    crate::node::Span::EMPTY,
                ));
            }
            if let Some(e) = init {
                check_expr(m, f, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                check_expr(m, f, e, errors);
            }
        }
        TirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            check_expr(m, f, cond, errors);
            for s in then_body.iter().chain(else_body) {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            check_expr(m, f, cond, errors);
            for s in body {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Try {
            body,
            catch_local,
            catch_body,
        } => {
            if catch_local.0 as usize >= f.locals.len() {
                errors.push(VerifyError::new(
                    format!(
                        "Try binds catch LocalId({}), which is out of range",
                        catch_local.0
                    ),
                    crate::node::Span::EMPTY,
                ));
            }
            for s in body.iter().chain(catch_body) {
                check_stmt(m, f, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
    }
}

fn check_expr(m: &TirModule, f: &TirFunction, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    check_ty(m, e.ty, e, errors);
    check_res(m, f, e, errors);

    match &e.kind {
        TirExprKind::Binary { lhs, rhs, .. } => {
            check_expr(m, f, lhs, errors);
            check_expr(m, f, rhs, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            check_expr(m, f, operand, errors)
        }
        TirExprKind::Field { object, .. } => check_expr(m, f, object, errors),
        TirExprKind::Index { object, index } => {
            check_expr(m, f, object, errors);
            check_expr(m, f, index, errors);
        }
        TirExprKind::Call { callee, args } => {
            check_expr(m, f, callee, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            check_expr(m, f, recv, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::Assign { target, value } => {
            check_expr(m, f, target, errors);
            check_expr(m, f, value, errors);
        }
        TirExprKind::TupleLit(xs) => {
            for x in xs {
                check_expr(m, f, x, errors);
            }
        }
        TirExprKind::RecordLit { fields } => {
            for (_, v) in fields {
                check_expr(m, f, v, errors);
            }
        }
        TirExprKind::ArrayLit(els) => {
            for el in els {
                match el {
                    crate::node::TirArrayEl::Expr(x) | crate::node::TirArrayEl::Spread(x) => {
                        check_expr(m, f, x, errors)
                    }
                    crate::node::TirArrayEl::Hole => {}
                }
            }
        }
        TirExprKind::ObjectLit { entries } => {
            for entry in entries {
                match entry {
                    crate::node::TirObjectEntry::Field { value, .. }
                    | crate::node::TirObjectEntry::Spread(value) => check_expr(m, f, value, errors),
                }
            }
        }
        TirExprKind::Closure { func, .. } => {
            if m.function(*func).is_none() {
                errors.push(VerifyError::new(
                    format!("Closure names FnId({}), which has no entry", func.0),
                    e.span,
                ));
            }
        }
        TirExprKind::New { class, args } => {
            if m.class(*class).is_none() {
                errors.push(VerifyError::new(
                    format!("New names ClassId({}), which has no entry", class.0),
                    e.span,
                ));
            }
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::MakeVariant { args } => {
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::Await { future } => check_expr(m, f, future, errors),
        TirExprKind::Yield { value, .. } => {
            if let Some(v) = value {
                check_expr(m, f, v, errors);
            }
        }
        TirExprKind::Discriminant { value } => check_expr(m, f, value, errors),
        TirExprKind::VariantPayload { value, .. } => check_expr(m, f, value, errors),
        TirExprKind::TypeTest { value, class } => {
            if m.class(*class).is_none() {
                errors.push(VerifyError::new(
                    format!("TypeTest names ClassId({}), which has no entry", class.0),
                    e.span,
                ));
            }
            check_expr(m, f, value, errors);
        }
        TirExprKind::Select {
            cond,
            then_val,
            else_val,
        } => {
            check_expr(m, f, cond, errors);
            check_expr(m, f, then_val, errors);
            check_expr(m, f, else_val, errors);
        }
        TirExprKind::Seq { stmts, value } => {
            for s in stmts {
                check_stmt(m, f, s, errors);
            }
            check_expr(m, f, value, errors);
        }
        TirExprKind::ObjectKeys { operand } => check_expr(m, f, operand, errors),
        TirExprKind::IterInit { source, .. } => check_expr(m, f, source, errors),
        TirExprKind::SuperCall { args } | TirExprKind::SuperMethodCall { args, .. } => {
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::RangeLit { start, end, .. } => {
            check_expr(m, f, start, errors);
            check_expr(m, f, end, errors);
        }
        TirExprKind::DecimalLit(_) | TirExprKind::BigIntLit(_) => {}
        TirExprKind::ObjectRest { object, .. } => check_expr(m, f, object, errors),
        TirExprKind::ExtensionCall { recv, args, .. } => {
            check_expr(m, f, recv, errors);
            for a in args {
                check_expr(m, f, a.value(), errors);
            }
        }
        TirExprKind::IntLit(_)
        | TirExprKind::FloatLit(_)
        | TirExprKind::BoolLit(_)
        | TirExprKind::StrLit(_)
        | TirExprKind::CharLit(_)
        | TirExprKind::NullLit
        | TirExprKind::Var => {}
    }
}
