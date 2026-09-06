//! Type ↔ operation coherence.
//!
//! A missing type costs performance; a wrong one is a miscompile. Nothing in
//! the pipeline looks for the second today.

use super::VerifyError;
use crate::node::{TirBinOp, TirExpr, TirExprKind, TirFunction, TirModule, TirStmt};
use crate::resolution::Resolution;
use crate::ty::{BackendTy, SigId};

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
        TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(m, e, errors),
        TirStmt::Let { ty, init, .. } => {
            if let Some(e) = init {
                walk_expr(m, e, errors);
                check_let(m, *ty, e, errors);
            }
        }
        TirStmt::Return(v) => {
            if let Some(e) = v {
                walk_expr(m, e, errors);
                check_return(m, f, e, errors);
            } else {
                // Bare Return(None) requires function to return Void
                check_return_none(m, f, errors);
            }
        }
        TirStmt::If { cond, then_body, else_body } => {
            walk_expr(m, cond, errors);
            check_condition(m, "if", cond, errors);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Loop { cond, body } => {
            walk_expr(m, cond, errors);
            check_condition(m, "loop", cond, errors);
            for s in body {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Try { body, catch_body, .. } => {
            for s in body.iter().chain(catch_body) {
                walk_stmt(m, f, s, errors);
            }
        }
        TirStmt::Break | TirStmt::Continue => {}
    }
}

fn walk_expr(m: &TirModule, e: &TirExpr, errors: &mut Vec<VerifyError>) {
    match &e.kind {
        TirExprKind::Binary { op, lhs, rhs } => {
            walk_expr(m, lhs, errors);
            walk_expr(m, rhs, errors);
            check_binary(m, e, *op, lhs, rhs, errors);
        }
        TirExprKind::Field { object, .. } => {
            walk_expr(m, object, errors);
            check_field(m, e, object, errors);
        }
        TirExprKind::Index { object, index } => {
            walk_expr(m, object, errors);
            walk_expr(m, index, errors);
            check_index(m, e, object, errors);
        }
        TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
            walk_expr(m, operand, errors)
        }
        TirExprKind::Call { callee, args } => {
            walk_expr(m, callee, errors);
            for a in args {
                walk_expr(m, a, errors);
            }
            check_direct_call(m, e, args, errors);
        }
        TirExprKind::MethodCall { recv, args, .. } => {
            walk_expr(m, recv, errors);
            for a in args {
                walk_expr(m, a, errors);
            }
            check_method_call(m, e, recv, args, errors);
        }
        TirExprKind::Assign { target, value } => {
            walk_expr(m, target, errors);
            walk_expr(m, value, errors);
        }
        TirExprKind::ArrayLit(xs) | TirExprKind::TupleLit(xs) => {
            for x in xs {
                walk_expr(m, x, errors);
            }
        }
        TirExprKind::ObjectLit { fields } => {
            for (_, v) in fields {
                walk_expr(m, v, errors);
            }
        }
        TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
            for a in args {
                walk_expr(m, a, errors);
            }
        }
        TirExprKind::Select { cond, then_val, else_val } => {
            walk_expr(m, cond, errors);
            walk_expr(m, then_val, errors);
            walk_expr(m, else_val, errors);
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
        | TirExprKind::Var => {}
    }
}

fn is_comparison(op: TirBinOp) -> bool {
    matches!(
        op,
        TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
    )
}

fn check_binary(
    m: &TirModule,
    e: &TirExpr,
    op: TirBinOp,
    lhs: &TirExpr,
    rhs: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    if is_comparison(op) {
        if e.ty != BackendTy::Bool {
            errors.push(VerifyError::new(
                format!("comparison {op:?} must produce Bool, node says {:?}", e.ty),
                e.span,
            ));
        }
        return;
    }

    let l = lhs.ty.non_nullable(&m.types);
    let r = rhs.ty.non_nullable(&m.types);

    // Dynamic operands make the operation generic; nothing to prove.
    if matches!(l, BackendTy::Dynamic(_)) || matches!(r, BackendTy::Dynamic(_)) {
        return;
    }

    if l != r {
        errors.push(VerifyError::new(
            format!(
                "{op:?} mixes {:?} and {:?}; an explicit Cast is required",
                l, r
            ),
            e.span,
        ));
        return;
    }

    // int / int is the one arithmetic case whose result leaves the operand
    // class, and `varn_core::numeric` is where that rule lives.
    let expected = if op == TirBinOp::Div && l == BackendTy::Int {
        BackendTy::Float
    } else {
        l
    };

    if e.ty != expected {
        errors.push(VerifyError::new(
            format!(
                "{op:?} on {:?} produces {:?}, node says {:?}",
                l, expected, e.ty
            ),
            e.span,
        ));
    }
}

fn check_field(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    let Resolution::FieldSlot(slot) = &e.res else {
        return;
    };
    let BackendTy::Class(c) = object.ty.non_nullable(&m.types) else {
        return; // well-formedness already reported this
    };
    let Some(field) = m.class(c).and_then(|ci| ci.field_at(*slot)) else {
        return; // ditto
    };
    if e.ty != field.ty {
        errors.push(VerifyError::new(
            format!(
                "field `{}` is declared {:?}, node says {:?}",
                field.name, field.ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_index(m: &TirModule, e: &TirExpr, object: &TirExpr, errors: &mut Vec<VerifyError>) {
    if let BackendTy::Array(el) = object.ty.non_nullable(&m.types) {
        let elem = m.types.get(el);
        if e.ty != elem {
            errors.push(VerifyError::new(
                format!("indexing an array of {:?} must produce {:?}, node says {:?}", elem, elem, e.ty),
                e.span,
            ));
        }
    }
}

fn check_direct_call(
    m: &TirModule,
    e: &TirExpr,
    args: &[TirExpr],
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::DirectFn(f) = &e.res else {
        return;
    };
    let Some(func) = m.function(*f) else {
        return;
    };
    let Some(sig) = m.signature(func.sig) else {
        return;
    };
    if args.len() != sig.arity() {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` passes {} arguments, signature takes {}",
                func.name,
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return;
    }
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if matches!(a.ty, BackendTy::Dynamic(_)) {
            continue;
        }
        if a.ty != *p {
            errors.push(VerifyError::new(
                format!(
                    "call to `{}`: argument {i} is {:?}, parameter is {:?}",
                    func.name, a.ty, p
                ),
                e.span,
            ));
        }
    }
    if e.ty != sig.return_ty && !matches!(e.ty, BackendTy::Dynamic(_)) {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` returns {:?}, node says {:?}",
                func.name, sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_method_call(
    m: &TirModule,
    e: &TirExpr,
    recv: &TirExpr,
    args: &[TirExpr],
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::VtableSlot(slot) = &e.res else {
        return;
    };
    let BackendTy::Class(c) = recv.ty.non_nullable(&m.types) else {
        return; // well-formedness already reported this
    };
    let Some(class_info) = m.class(c) else {
        return; // ditto
    };
    let Some(vtable_entry) = class_info.method_at(*slot) else {
        return; // ditto
    };
    let Some(sig) = m.signature(vtable_entry.sig) else {
        return; // ditto
    };

    // Check arity
    if args.len() != sig.arity() {
        errors.push(VerifyError::new(
            format!(
                "method call passes {} arguments, signature takes {}",
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return;
    }

    // Check argument types
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if matches!(a.ty, BackendTy::Dynamic(_)) {
            continue;
        }
        if a.ty != *p {
            errors.push(VerifyError::new(
                format!(
                    "method call: argument {i} is {:?}, parameter is {:?}",
                    a.ty, p
                ),
                e.span,
            ));
        }
    }

    // Check return type
    if e.ty != sig.return_ty && !matches!(e.ty, BackendTy::Dynamic(_)) {
        errors.push(VerifyError::new(
            format!(
                "method call returns {:?}, node says {:?}",
                sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}

fn check_condition(
    m: &TirModule,
    context: &str,
    cond: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if condition is Dynamic
    if matches!(cond.ty, BackendTy::Dynamic(_)) {
        return;
    }

    if cond.ty != BackendTy::Bool {
        errors.push(VerifyError::new(
            format!("{context} condition must be Bool, node says {:?}", cond.ty),
            cond.span,
        ));
    }
}

fn check_let(
    m: &TirModule,
    declared_ty: BackendTy,
    init: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if either type is Dynamic
    if matches!(declared_ty, BackendTy::Dynamic(_))
        || matches!(init.ty, BackendTy::Dynamic(_))
    {
        return;
    }

    if declared_ty != init.ty {
        errors.push(VerifyError::new(
            format!(
                "let binding declares {:?}, initializer is {:?}",
                declared_ty, init.ty
            ),
            init.span,
        ));
    }
}

fn check_return(
    m: &TirModule,
    f: &TirFunction,
    returned: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if either type is Dynamic
    if matches!(f.return_ty, BackendTy::Dynamic(_))
        || matches!(returned.ty, BackendTy::Dynamic(_))
    {
        return;
    }

    if f.return_ty != returned.ty {
        errors.push(VerifyError::new(
            format!(
                "function `{}` declares return type {:?}, returned {:?}",
                f.name, f.return_ty, returned.ty
            ),
            returned.span,
        ));
    }
}

fn check_return_none(
    m: &TirModule,
    f: &TirFunction,
    errors: &mut Vec<VerifyError>,
) {
    // Skip if function return type is Dynamic
    if matches!(f.return_ty, BackendTy::Dynamic(_)) {
        return;
    }

    if f.return_ty != BackendTy::Void {
        errors.push(VerifyError::new(
            format!(
                "function `{}` declares return type {:?}, bare return is Void",
                f.name, f.return_ty
            ),
            crate::node::Span::EMPTY,
        ));
    }
}
