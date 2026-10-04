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
    let label = format!("call to `{}`", func.name);
    if !check_args(m, sig, args, &label, e, errors) {
        return;
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
    if !check_args(m, sig, args, "method call", e, errors) {
        return;
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

/// Arity and per-argument assignability against `sig`. A rest signature takes
/// at least its fixed parameters, and each trailing argument is one element of
/// the rest array. `false` when the arity is wrong: nothing else is checked.
fn check_args(
    m: &TirModule,
    sig: &crate::tables::Signature,
    args: &[TirArg],
    label: &str,
    e: &TirExpr,
    errors: &mut Vec<VerifyError>,
) -> bool {
    let fixed = if sig.has_rest {
        sig.arity().saturating_sub(1)
    } else {
        sig.arity()
    };
    let arity_ok = if sig.has_rest {
        args.len() >= fixed
    } else {
        args.len() == fixed
    };
    if !arity_ok {
        errors.push(VerifyError::new(
            format!(
                "{label} passes {} arguments, signature takes {}",
                args.len(),
                sig.arity()
            ),
            e.span,
        ));
        return false;
    }
    let element = match sig.params.get(fixed) {
        Some(BackendTy::Array(elem)) if sig.has_rest => Some(m.types.get(*elem)),
        _ => None,
    };
    for (i, a) in args.iter().enumerate() {
        let expected = if i < fixed {
            Some(sig.params[i])
        } else {
            element
        };
        let Some(p) = expected else {
            continue;
        };
        if !assignable(m, a.value().ty, p) {
            errors.push(VerifyError::new(
                format!(
                    "{label}: argument {i} is {:?}, parameter is {:?}",
                    a.value().ty,
                    p
                ),
                e.span,
            ));
        }
    }
    true
}
