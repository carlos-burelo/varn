use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{CheckerTyTable, Type};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;

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
        varn_core::TypeKind::Primitive(varn_core::LangPrimitive::Never)
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
                return Type::primitive(
                    varn_core::LangPrimitive::Int,
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
    let ty = &ty.apparent(table);
    if let TypeKind::Named(name, _) = table.get(ty.0) {
        match interner.resolve(name) {
            n if n == varn_core::LangPrimitive::Str.name() => return Type::Str,
            n if n == varn_core::LangPrimitive::Int.name() => return Type::Int,
            n if n == varn_core::LangPrimitive::Float.name() => return Type::Float,
            n if n == varn_core::LangPrimitive::Bool.name() => return Type::Bool,
            n if n == varn_core::LangPrimitive::Decimal.name() => return Type::Decimal,
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
            let (l, r) = crate::binder::type_inference::adopt_literal_operands(
                checker.ast_arena,
                left,
                right,
                l,
                r,
                &checker.ty_table,
            );
            match op {
                BinaryOp::Add => {
                    if matches!(checker.ty_table.get(l.0), TypeKind::Primitive(varn_core::LangPrimitive::Str))
                        || matches!(checker.ty_table.get(r.0), TypeKind::Primitive(varn_core::LangPrimitive::Str))
                    {
                        return Type::Str;
                    }
                    crate::binder::type_inference::numeric_binary_type(&l, &r, &checker.ty_table)
                        .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                    crate::binder::type_inference::numeric_binary_type(&l, &r, &checker.ty_table)
                        .unwrap_or_else(|| Type::Dynamic.tainted())
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr => {
                    if l.is_int() && r.is_int() {
                        return Type::primitive(
                            varn_core::LangPrimitive::Int,
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
