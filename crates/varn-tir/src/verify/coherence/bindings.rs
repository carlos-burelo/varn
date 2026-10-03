use super::assign::assignable;
use crate::node::{Span, TirExpr, TirFunction, TirModule};
use crate::ty::BackendTy;
use crate::verify::VerifyError;

pub(super) fn check_condition(_m: &TirModule, context: &str, cond: &TirExpr, errors: &mut Vec<VerifyError>) {
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

pub(super) fn check_let(m: &TirModule, declared_ty: BackendTy, init: &TirExpr, errors: &mut Vec<VerifyError>) {
    // The initializer must be assignable TO the declared type
    if !assignable(m, init.ty, declared_ty) {
        errors.push(VerifyError::new(
            format!(
                "let binding declares {:?}, initializer is {:?}",
                declared_ty, init.ty
            ),
            init.span,
        ));
    }
}

pub(super) fn check_return(m: &TirModule, f: &TirFunction, returned: &TirExpr, errors: &mut Vec<VerifyError>) {
    // The returned value must be assignable TO the function's return_ty
    if !assignable(m, returned.ty, f.return_ty) {
        errors.push(VerifyError::new(
            format!(
                "function `{}` declares return type {:?}, returned {:?}",
                f.name, f.return_ty, returned.ty
            ),
            returned.span,
        ));
    }
}

pub(super) fn check_return_none(_m: &TirModule, f: &TirFunction, errors: &mut Vec<VerifyError>) {
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
            Span::EMPTY,
        ));
    }
}
