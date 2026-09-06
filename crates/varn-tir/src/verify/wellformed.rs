//! Every handle points at something that exists; every slot is in range.

use super::VerifyError;
use crate::node::{TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::BackendTy;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    check_function(m, &m.top_level, errors);
    for f in &m.functions {
        check_function(m, f, errors);
    }
}

fn check_function(m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
    if m.signature(f.sig).is_none() {
        errors.push(VerifyError::new(
            format!("function `{}` names SigId({}), which has no entry", f.name, f.sig.0),
            crate::node::Span::EMPTY,
        ));
    }
    for s in &f.body {
        check_stmt(m, s, errors);
    }
}

fn check_stmt(m: &TirModule, s: &TirStmt, errors: &mut Vec<VerifyError>) {
    match s {
        TirStmt::Expr(e) | TirStmt::Throw(e) => check_expr(m, e, errors),
        TirStmt::Let { init, .. } => {
            if let Some(e) = init {
                check_expr(m, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                check_expr(m, e, errors);
            }
        }
        TirStmt::If { cond, then_body, else_body } => {
            check_expr(m, cond, errors);
            for s in then_body.iter().chain(else_body) {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            check_expr(m, cond, errors);
            for s in body {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Try { body, catch_body, .. } => {
            for s in body.iter().chain(catch_body) {
                check_stmt(m, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue => {}
    }
}

fn check_expr(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    check_ty(m, e.ty, e, errors);
    check_res(m, e, errors);

    match &e.kind {
        TirExprKind::Binary { lhs, rhs, .. } => {
            check_expr(m, lhs, errors);
            check_expr(m, rhs, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            check_expr(m, operand, errors)
        }
        TirExprKind::Field { object, .. } => check_expr(m, object, errors),
        TirExprKind::Index { object, index } => {
            check_expr(m, object, errors);
            check_expr(m, index, errors);
        }
        TirExprKind::Call { callee, args } => {
            check_expr(m, callee, errors);
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            check_expr(m, recv, errors);
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::Assign { target, value } => {
            check_expr(m, target, errors);
            check_expr(m, value, errors);
        }
        TirExprKind::ArrayLit(xs) | TirExprKind::TupleLit(xs) => {
            for x in xs {
                check_expr(m, x, errors);
            }
        }
        TirExprKind::ObjectLit { fields } => {
            for (_, v) in fields {
                check_expr(m, v, errors);
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
                check_expr(m, a, errors);
            }
        }
        TirExprKind::MakeVariant { args } => {
            for a in args {
                check_expr(m, a, errors);
            }
        }
        TirExprKind::Select { cond, then_val, else_val } => {
            check_expr(m, cond, errors);
            check_expr(m, then_val, errors);
            check_expr(m, else_val, errors);
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

/// Every handle inside a type points at an entry that exists.
fn check_ty(m: &TirModule, ty: BackendTy, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let bad = |what: &str, errors: &mut Vec<VerifyError>| {
        errors.push(VerifyError::new(
            format!("type names {what}, which has no entry"),
            e.span,
        ));
    };
    match ty {
        BackendTy::Class(c) if m.class(c).is_none() => bad(&format!("ClassId({})", c.0), errors),
        BackendTy::Enum(en) if m.enum_info(en).is_none() => {
            bad(&format!("EnumId({})", en.0), errors)
        }
        BackendTy::Fn(s) if m.signature(s).is_none() => bad(&format!("SigId({})", s.0), errors),
        BackendTy::Array(t) | BackendTy::Set(t) | BackendTy::Nullable(t) => {
            if !m.types.contains(t) {
                bad(&format!("TyId({})", t.0), errors);
            }
        }
        BackendTy::Map(k, v) => {
            if !m.types.contains(k) {
                bad(&format!("TyId({})", k.0), errors);
            }
            if !m.types.contains(v) {
                bad(&format!("TyId({})", v.0), errors);
            }
        }
        BackendTy::Tuple(l) => {
            if !m.types.contains_list(l) {
                bad(&format!("TyListId({})", l.0), errors);
            }
        }
        _ => {}
    }
}

/// Every slot is in range of the table it claims to index.
fn check_res(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    let receiver_class = |recv: &TirExpr| match recv.ty.non_nullable(&m.types) {
        BackendTy::Class(c) => Some(c),
        _ => None,
    };

    match (&e.res, &e.kind) {
        (Resolution::FieldSlot(slot), TirExprKind::Field { object, .. }) => {
            match receiver_class(object) {
                Some(c) => {
                    let ok = m.class(c).and_then(|ci| ci.field_at(*slot)).is_some();
                    if !ok {
                        errors.push(VerifyError::new(
                            format!(
                                "field slot {slot} is out of range for class ClassId({})",
                                c.0
                            ),
                            e.span,
                        ));
                    }
                }
                None => errors.push(VerifyError::new(
                    format!("field slot {slot} on a receiver that is not a class"),
                    e.span,
                )),
            }
        }
        (Resolution::VtableSlot(slot), TirExprKind::MethodCall { recv, .. }) => {
            match receiver_class(recv) {
                Some(c) => {
                    let ok = m.class(c).and_then(|ci| ci.method_at(*slot)).is_some();
                    if !ok {
                        errors.push(VerifyError::new(
                            format!(
                                "vtable slot {slot} is out of range for class ClassId({})",
                                c.0
                            ),
                            e.span,
                        ));
                    }
                }
                None => errors.push(VerifyError::new(
                    format!("vtable slot {slot} on a receiver that is not a class"),
                    e.span,
                )),
            }
        }
        (Resolution::GlobalSlot(slot), _) => {
            if *slot as usize >= m.globals.len() {
                errors.push(VerifyError::new(
                    format!("global slot {slot} is out of range"),
                    e.span,
                ));
            }
        }
        (Resolution::DirectFn(f), _) => {
            if m.function(*f).is_none() {
                errors.push(VerifyError::new(
                    format!("DirectFn names FnId({}), which has no entry", f.0),
                    e.span,
                ));
            }
        }
        (Resolution::EnumVariant { enum_id, tag }, _) => {
            let ok = m
                .enum_info(*enum_id)
                .and_then(|ei| ei.variant_at(*tag))
                .is_some();
            if !ok {
                errors.push(VerifyError::new(
                    format!("EnumId({}) has no variant with tag {tag}", enum_id.0),
                    e.span,
                ));
            }
        }
        _ => {}
    }
}
