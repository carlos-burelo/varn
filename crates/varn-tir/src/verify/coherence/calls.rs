use super::assign::assignable;
use crate::node::{TirArg, TirExpr};
use crate::resolution::Resolution;
use crate::ty::BackendTy;
use crate::verify::VerifyError;
use crate::TirModule;

/// Arity and per-argument types can only be checked against a positional list
/// with no spread. A spread contributes an unknown count; a named argument is
/// matched by label, not position — both are left to a later rule.
fn is_positional(args: &[TirArg]) -> bool {
    args.iter().all(|a| matches!(a, TirArg::Expr(_)))
}

pub(super) fn check_direct_call(
    m: &TirModule,
    e: &TirExpr,
    args: &[TirArg],
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
    if !is_positional(args) {
        return;
    }
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
    // Each argument must be assignable TO its parameter
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if !assignable(m, a.value().ty, *p) {
            errors.push(VerifyError::new(
                format!(
                    "call to `{}`: argument {i} is {:?}, parameter is {:?}",
                    func.name,
                    a.value().ty,
                    p
                ),
                e.span,
            ));
        }
    }
    // The signature's return type must be assignable TO what the node claims
    if !assignable(m, sig.return_ty, e.ty) {
        errors.push(VerifyError::new(
            format!(
                "call to `{}` returns {:?}, node says {:?}",
                func.name, sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}

pub(super) fn check_method_call(
    m: &TirModule,
    e: &TirExpr,
    recv: &TirExpr,
    args: &[TirArg],
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

    if !is_positional(args) {
        return;
    }

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

    // Each argument must be assignable TO its parameter
    for (i, (a, p)) in args.iter().zip(&sig.params).enumerate() {
        if !assignable(m, a.value().ty, *p) {
            errors.push(VerifyError::new(
                format!(
                    "method call: argument {i} is {:?}, parameter is {:?}",
                    a.value().ty,
                    p
                ),
                e.span,
            ));
        }
    }

    // The signature's return type must be assignable TO what the node claims
    if !assignable(m, sig.return_ty, e.ty) {
        errors.push(VerifyError::new(
            format!(
                "method call returns {:?}, node says {:?}",
                sig.return_ty, e.ty
            ),
            e.span,
        ));
    }
}
