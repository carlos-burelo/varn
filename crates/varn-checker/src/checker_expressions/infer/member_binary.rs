use rustc_hash::FxHashMap;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::{build_generic_mapping, map_generics_cached};
use crate::types::Type;
use varn_core::ast::{Expr, ExprKind};
use varn_core::TypeKind;
use varn_core::TypeTag;

use super::super::helpers::base_type;

pub(super) fn infer_member_type(
    checker: &mut Checker<'_>,
    expr: &Expr,
    object: &Expr,
    property: &Expr,
    bind: &BindResult,
) -> Type {
    let obj_ty_raw = checker.infer_type(object, bind);
    let obj_ty = obj_ty_raw.non_nullified();
    let obj_ty = if matches!(
        obj_ty.0,
        varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Never)
    ) {
        obj_ty_raw
    } else {
        obj_ty
    };

    let ExprKind::Identifier { name: prop_name } = &property.kind else {
        return crate::binder::infer_expr_type(
            expr,
            Some(&crate::binder::BindView::new(bind, checker.resolver)),
        );
    };

    match &obj_ty.0 {
        TypeKind::Named(class_name, _origin) | TypeKind::Generic(class_name, _, _origin) => {
            let mapping = if let TypeKind::Generic(_, type_args, _orig) = &obj_ty.0 {
                build_generic_mapping(class_name.as_ref(), type_args, checker, bind)
            } else {
                FxHashMap::default()
            };

            if let Some(res) = checker.find_member_info(&obj_ty, prop_name.as_ref(), bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return map_generics_cached(checker, &m_ty, &mapping);
                }
            }
        }

        TypeKind::Array(elem) => {
            if prop_name.as_ref() == varn_core::MemberKey::Length.as_str() {
                return Type::intrinsic(TypeTag::Int);
            }

            let effective_elem = if prop_name.as_ref() == "flat" {
                match &elem.0 {
                    varn_core::TypeKind::Array(inner) => *inner.clone(),
                    _ => *elem.clone(),
                }
            } else {
                *elem.clone()
            };
            let mapping = build_generic_mapping(
                varn_core::IntrinsicType::Array.as_str(),
                &[effective_elem],
                checker,
                bind,
            );
            if let Some(res) = checker.find_member_info(&obj_ty, prop_name.as_ref(), bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return map_generics_cached(checker, &m_ty, &mapping);
                }
            }
        }
        _ => {
            if let Some(res) = checker.find_member_info(&obj_ty, prop_name.as_ref(), bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return m_ty;
                }
            }
        }
    }

    crate::binder::infer_expr_type(
        expr,
        Some(&crate::binder::BindView::new(bind, checker.resolver)),
    )
}

pub(crate) fn normalize_for_binary(ty: &Type) -> Type {
    if let TypeKind::Named(name, _) = &ty.0 {
        match name.as_ref() {
            n if n == varn_core::IntrinsicType::Str.as_str() => return Type::Str,
            n if n == varn_core::IntrinsicType::Int.as_str() => return Type::Int,
            n if n == varn_core::IntrinsicType::Float.as_str() => return Type::Float,
            n if n == varn_core::IntrinsicType::Bool.as_str() => return Type::Bool,
            n if n == varn_core::IntrinsicType::Decimal.as_str() => return Type::Decimal,
            _ => {}
        }
    }
    ty.clone()
}

pub(super) fn infer_binary_type(
    checker: &mut Checker<'_>,
    op: &varn_core::ast::operators::BinaryOp,
    left: &Expr,
    right: &Expr,
    bind: &BindResult,
) -> Type {
    use varn_core::ast::operators::BinaryOp;

    match op {
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::LtEq
        | BinaryOp::GtEq
        | BinaryOp::Instanceof
        | BinaryOp::In => Type::Bool,
        _ => {
            let l = normalize_for_binary(&base_type(&checker.infer_type(left, bind)));
            let r = normalize_for_binary(&base_type(&checker.infer_type(right, bind)));
            if l.is_dynamic() || r.is_dynamic() {
                return Type::Dynamic.tainted();
            }
            match op {
                BinaryOp::Add => {
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Str))
                        || matches!(r.0, TypeKind::Intrinsic(TypeTag::Str))
                    {
                        return Type::Str;
                    }
                    crate::binder::type_inference::numeric_binary_type(*op, &l, &r)
                        .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                    crate::binder::type_inference::numeric_binary_type(*op, &l, &r)
                        .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr => {
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Int))
                        && matches!(r.0, TypeKind::Intrinsic(TypeTag::Int))
                    {
                        return Type::intrinsic(TypeTag::Int);
                    }
                    Type::Dynamic.tainted()
                }
                _ => Type::Dynamic.tainted(),
            }
        }
    }
}
