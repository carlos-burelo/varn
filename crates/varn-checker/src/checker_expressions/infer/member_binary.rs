use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{CheckerTyTable, Type};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;
use varn_core::TypeTag;

use super::super::helpers::base_type;

pub(super) fn infer_member_type(
    checker: &mut Checker<'_>,
    expr: ExprId,
    object: ExprId,
    property: ExprId,
    bind: &BindResult,
) -> Type {
    let arena = checker.ast_arena;
    let obj_ty_raw = checker.infer_type(object, bind);
    let obj_ty = obj_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut checker.ty_table));
    let obj_ty = if matches!(
        checker.ty_table.get(obj_ty.0),
        varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Never)
    ) {
        obj_ty_raw
    } else {
        obj_ty
    };

    let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
        return crate::binder::infer_expr_type(
            expr,
            arena,
            Some(&crate::binder::BindView::new(bind, checker.resolver)),
            &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
        );
    };

    let prop_name_str = bind.interner.resolve(*prop_name);
    let obj_kind = checker.ty_table.get(obj_ty.0);
    match obj_kind {
        TypeKind::Array(_elem) => {
            if prop_name_str == varn_core::MemberKey::Length.as_str() {
                return Type::intrinsic(
                    TypeTag::Int,
                    &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                );
            }
            if let Some(res) = checker.find_member_info(&obj_ty, prop_name_str, bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return m_ty;
                }
            }
        }
        _ => {
            if let Some(res) = checker.find_member_info(&obj_ty, prop_name_str, bind) {
                let m_ty = res.0;
                if !m_ty.is_dynamic() {
                    return m_ty;
                }
            }
        }
    }

    crate::binder::infer_expr_type(
        expr,
        arena,
        Some(&crate::binder::BindView::new(bind, checker.resolver)),
        &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
    )
}

pub(crate) fn normalize_for_binary(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &varn_core::AtomInterner,
) -> Type {
    if let TypeKind::Named(name, _) = table.get(ty.0) {
        match interner.resolve(name) {
            n if n == varn_core::IntrinsicType::Str.as_str() => return Type::Str,
            n if n == varn_core::IntrinsicType::Int.as_str() => return Type::Int,
            n if n == varn_core::IntrinsicType::Float.as_str() => return Type::Float,
            n if n == varn_core::IntrinsicType::Bool.as_str() => return Type::Bool,
            n if n == varn_core::IntrinsicType::Decimal.as_str() => return Type::Decimal,
            _ => {}
        }
    }
    *ty
}

pub(super) fn infer_binary_type(
    checker: &mut Checker<'_>,
    op: varn_core::ast::operators::BinaryOp,
    left: ExprId,
    right: ExprId,
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
            let l_raw = base_type(&checker.infer_type(left, bind));
            let r_raw = base_type(&checker.infer_type(right, bind));
            let l = normalize_for_binary(&l_raw, &checker.ty_table, &bind.interner);
            let r = normalize_for_binary(&r_raw, &checker.ty_table, &bind.interner);
            if l.is_dynamic() || r.is_dynamic() {
                return Type::Dynamic.tainted();
            }
            match op {
                BinaryOp::Add => {
                    if matches!(checker.ty_table.get(l.0), TypeKind::Intrinsic(TypeTag::Str))
                        || matches!(checker.ty_table.get(r.0), TypeKind::Intrinsic(TypeTag::Str))
                    {
                        return Type::Str;
                    }
                    crate::binder::type_inference::numeric_binary_type(
                        op,
                        &l,
                        &r,
                        &checker.ty_table,
                    )
                    .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                    crate::binder::type_inference::numeric_binary_type(
                        op,
                        &l,
                        &r,
                        &checker.ty_table,
                    )
                    .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr => {
                    if l.is_int() && r.is_int() {
                        return Type::intrinsic(
                            TypeTag::Int,
                            &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                        );
                    }
                    Type::Dynamic.tainted()
                }
                _ => Type::Dynamic.tainted(),
            }
        }
    }
}
