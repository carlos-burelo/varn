use super::assign::assignable;
use crate::node::{TirArg, TirExpr};
use crate::resolution::Resolution;
use crate::ty::BackendTy;
use crate::verify::VerifyError;
use crate::TirModule;

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
        return;
    };
    let Some(class_info) = m.class(c) else {
        return;
    };
    let Some(vtable_entry) = class_info.method_at(*slot) else {
        return;
    };
    let Some(sig) = m.signature(vtable_entry.sig) else {
        return;
    };

    if !is_positional(args) {
        return;
    }
    if !check_args(m, sig, args, "method call", e, errors) {
        return;
    }

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
        Some(BackendTy::Int) | Some(BackendTy::Float) | Some(BackendTy::Bool) | Some(BackendTy::Char) | Some(BackendTy::Str) | Some(BackendTy::Bytes) | Some(BackendTy::Decimal) | Some(BackendTy::BigInt) | Some(BackendTy::Array(_)) | Some(BackendTy::Map(..)) | Some(BackendTy::Set(_)) | Some(BackendTy::Tuple(_)) | Some(BackendTy::Class(_)) | Some(BackendTy::Enum(_)) | Some(BackendTy::Fn(_)) | Some(BackendTy::Nullable(_)) | Some(BackendTy::Void) | Some(BackendTy::Never) | Some(BackendTy::Dynamic(_)) | None => None,
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
