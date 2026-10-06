use crate::node::{TirBinOp, TirExpr};
use crate::resolution::Resolution;
use crate::ty::BackendTy;
use crate::verify::VerifyError;
use crate::TirModule;

fn is_comparison(op: TirBinOp) -> bool {
    matches!(
        op,
        TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
    )
}

pub(super) fn check_binary(
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

    
    let expected = l;

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

pub(super) fn check_field(
    m: &TirModule,
    e: &TirExpr,
    object: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    let Resolution::FieldSlot(slot) = &e.res else {
        return;
    };
    let BackendTy::Class(c) = object.ty.non_nullable(&m.types) else {
        return; 
    };
    let Some(field) = m.class(c).and_then(|ci| ci.field_at(*slot)) else {
        return; 
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

pub(super) fn check_index(
    m: &TirModule,
    e: &TirExpr,
    object: &TirExpr,
    errors: &mut Vec<VerifyError>,
) {
    if let BackendTy::Array(el) = object.ty.non_nullable(&m.types) {
        let elem = m.types.get(el);
        if e.ty != elem {
            errors.push(VerifyError::new(
                format!(
                    "indexing an array of {:?} must produce {:?}, node says {:?}",
                    elem, elem, e.ty
                ),
                e.span,
            ));
        }
    }
}


pub(super) fn check_variant_payload(
    m: &TirModule,
    e: &TirExpr,
    value: &TirExpr,
    tag: u16,
    field: u16,
    errors: &mut Vec<VerifyError>,
) {
    let BackendTy::Enum(id) = value.ty.non_nullable(&m.types) else {
        return; 
    };
    let Some(variant) = m.enum_info(id).and_then(|ei| ei.variant_at(tag)) else {
        errors.push(VerifyError::new(
            format!(
                "VariantPayload names tag {tag}, which EnumId({}) has no variant for",
                id.0
            ),
            e.span,
        ));
        return;
    };
    let Some(&field_ty) = variant.payload.get(field as usize) else {
        errors.push(VerifyError::new(
            format!(
                "VariantPayload field {field} is out of range for variant `{}`",
                variant.name
            ),
            e.span,
        ));
        return;
    };
    if e.ty != field_ty {
        errors.push(VerifyError::new(
            format!(
                "variant `{}` field {field} is {:?}, node says {:?}",
                variant.name, field_ty, e.ty
            ),
            e.span,
        ));
    }
}
