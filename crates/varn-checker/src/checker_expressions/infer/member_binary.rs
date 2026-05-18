use rustc_hash::FxHashMap;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_generics::build_generic_mapping;
use crate::types::Type;
use varn_core::ast::{Expr, ExprKind};
use varn_core::TypeKind;
use varn_core::TypeTag;

use super::super::helpers::base_type;
use super::async_members::is_async_member;

pub(super) fn infer_member_type(
    checker: &mut Checker,
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
        return crate::binder::infer_expr_type(expr, Some(bind));
    };

    let wrap_async_method_type = |ty: Type| {
        if !is_async_member(&obj_ty, prop_name.as_ref(), bind) {
            return ty;
        }
        match &ty.0 {
            TypeKind::Fn(ft)
                if !matches!(&ft.return_type.0, TypeKind::Generic(name, _, _) if name.as_ref() == varn_core::IntrinsicType::Task.as_str())
                    && *ft.return_type != Type::Void
                    && !ft.return_type.is_dynamic() =>
            {
                let mut wrapped = ft.clone();
                wrapped.return_type = Box::new(Type::generic(
                    varn_core::IntrinsicType::Task.as_str().to_owned(),
                    vec![(*ft.return_type).clone()],
                ));
                Type::fn_(wrapped)
            }
            _ => ty,
        }
    };

    if let Some(res) = checker.find_member_info(&obj_ty, prop_name.as_ref(), bind) {
        return wrap_async_method_type(res.0);
    }

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
                    if mapping.is_empty() {
                        return wrap_async_method_type(m_ty);
                    }
                    return wrap_async_method_type(m_ty.map_generics(&mapping));
                }
            }
        }

        TypeKind::Array(elem) => {
            if prop_name.as_ref() == "length" {
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
                    return wrap_async_method_type(m_ty.map_generics(&mapping));
                }
            }
        }
        _ => {
            if let Some(res) = checker.find_member_info(&obj_ty, prop_name.as_ref(), bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return wrap_async_method_type(m_ty);
                }
            }
        }
    }

    crate::binder::infer_expr_type(expr, Some(bind))
}

pub(crate) fn normalize_for_binary(ty: &Type) -> Type {
    match &ty.0 {
        TypeKind::Named(name, _) => match name.as_ref() {
            n if n == varn_core::IntrinsicType::Str.as_str() => return Type::Str,
            n if n == varn_core::IntrinsicType::Int.as_str() => return Type::Int,
            n if n == varn_core::IntrinsicType::Float.as_str() => return Type::Float,
            n if n == varn_core::IntrinsicType::Bool.as_str() => return Type::Bool,
            n if n == varn_core::IntrinsicType::Decimal.as_str() => return Type::Decimal,
            _ => {}
        },
        TypeKind::LiteralStr(_) => return Type::Str,
        TypeKind::LiteralInt(_) => return Type::Int,
        TypeKind::LiteralFloat(_) => return Type::Float,
        TypeKind::LiteralBool(_) => return Type::Bool,
        _ => {}
    }
    ty.clone()
}

pub(super) fn infer_binary_type(
    checker: &mut Checker,
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
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Decimal))
                        || matches!(r.0, TypeKind::Intrinsic(TypeTag::Decimal))
                    {
                        return Type::Decimal;
                    }
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Float))
                        || matches!(r.0, TypeKind::Intrinsic(TypeTag::Float))
                    {
                        return Type::Float;
                    }
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Int))
                        && matches!(r.0, TypeKind::Intrinsic(TypeTag::Int))
                    {
                        return Type::intrinsic(TypeTag::Int);
                    }
                    Type::Dynamic.tainted()
                }
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Decimal))
                        || matches!(r.0, TypeKind::Intrinsic(TypeTag::Decimal))
                    {
                        return Type::Decimal;
                    }
                    if matches!(l.0, TypeKind::Intrinsic(TypeTag::Float))
                        || matches!(r.0, TypeKind::Intrinsic(TypeTag::Float))
                    {
                        return Type::Float;
                    }
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
