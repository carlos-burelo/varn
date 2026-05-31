use std::rc::Rc;
use varn_core::ast::operators::{BinaryOp, LogicalOp, UnaryOp};
use varn_core::ast::{ArrayEl, ArrowBody, Expr, ExprKind, MatchBody, ObjectProp, PropKey};

use super::inference_utils::{build_fn_type, build_method_params, infer_object_member_type};
pub use super::inference_utils::{pattern_lead_name, pattern_to_string, widen_literal};
use super::type_resolution::resolve_type_node;
use crate::types::{FunctionType, ObjectTypeMember, Type};
use varn_core::TypeKind;

pub fn infer_expr_type(expr: &Expr, ctx: Option<&dyn crate::types::TypeContext>) -> Type {
    match &expr.kind {
        ExprKind::IntLiteral { .. } => Type::Int,
        ExprKind::FloatLiteral { .. } => Type::Float,
        ExprKind::DecimalLiteral { .. } => Type::Decimal,
        ExprKind::BigIntLiteral { .. } => Type::BigInt,
        ExprKind::StrLiteral { value, .. } => Type::literal_str(value.to_string()),
        ExprKind::CharLiteral { .. } => Type::Char,
        ExprKind::BoolLiteral { .. } => Type::Bool,
        ExprKind::NullLiteral => Type::Null,
        ExprKind::Template { .. } => Type::Str,
        ExprKind::Paren { expression } => infer_expr_type(expression, ctx),
        ExprKind::As { type_ann, .. } => resolve_type_node(type_ann, ctx),
        ExprKind::Is { .. } => Type::Bool,
        ExprKind::Satisfies { expression, .. } => infer_expr_type(expression, ctx),
        ExprKind::Await { argument } => {
            let inner = infer_expr_type(argument, ctx);
            match &inner.0 {
                TypeKind::Generic(name, args, _)
                    if name.as_ref() == varn_core::IntrinsicType::Task.as_str()
                        && args.len() == 1 =>
                {
                    args[0].clone()
                }
                _ => inner,
            }
        }
        ExprKind::NonNull { expression } => infer_expr_type(expression, ctx),
        ExprKind::Logical {
            op, left, right, ..
        } => match op {
            LogicalOp::Nullish => {
                let rhs = infer_expr_type(right, ctx);
                if !rhs.is_dynamic() {
                    return rhs;
                }
                infer_expr_type(left, ctx)
            }
            LogicalOp::And | LogicalOp::Or => {
                let l = infer_expr_type(left, ctx);
                let r = infer_expr_type(right, ctx);
                if l.0 == r.0 {
                    l
                } else {
                    Type::Dynamic
                }
            }
        },
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => infer_member(object, property, *computed, ctx),
        ExprKind::Unary { op, operand, .. } => {
            let inner = infer_expr_type(operand, ctx);
            match op {
                UnaryOp::Minus | UnaryOp::Plus => inner,
                UnaryOp::Not => Type::Bool,
                UnaryOp::BitNot => match &inner.0 {
                    TypeKind::Intrinsic(varn_core::TypeTag::Int) => Type::Int,
                    _ => Type::Dynamic,
                },
                _ => Type::Dynamic,
            }
        }
        ExprKind::Binary {
            op, left, right, ..
        } => infer_binary(op, left, right, ctx),
        ExprKind::Array { elements } => {
            for el in elements {
                if let ArrayEl::Expr(first) = el {
                    let elem_ty = infer_expr_type(first, ctx);
                    if !elem_ty.is_dynamic() {
                        return Type::array(widen_literal(elem_ty));
                    }
                }
            }
            Type::Dynamic
        }
        ExprKind::Call { callee, .. } => {
            let callee_ty = infer_expr_type(callee, ctx);
            if let TypeKind::Fn(ft) = &callee_ty.0 {
                return *ft.return_type.clone();
            }
            Type::Dynamic
        }
        ExprKind::New {
            callee, type_args, ..
        } => infer_new(callee, type_args, ctx),
        ExprKind::Identifier { name } => ctx
            .and_then(|c| c.resolve_symbol(name))
            .unwrap_or(Type::Dynamic),
        ExprKind::This => ctx
            .and_then(|c| c.resolve_symbol("this"))
            .unwrap_or(Type::This),
        ExprKind::Arrow {
            params,
            return_type,
            body,
            ..
        } => {
            let mut inferred_ret = Type::Dynamic;
            if let ArrowBody::Expr(e) = body.as_ref() {
                inferred_ret = infer_expr_type(e, ctx);
            }
            build_fn_type(params, return_type, true, ctx, inferred_ret)
        }
        ExprKind::Function {
            params,
            return_type,
            is_generator,
            ..
        } => {
            let ret = if *is_generator {
                Type::generic("Generator", vec![Type::Dynamic])
            } else {
                Type::Dynamic
            };
            build_fn_type(params, return_type, false, ctx, ret)
        }
        ExprKind::Match { cases, .. } => {
            if let Some(first) = cases.first() {
                match &first.body {
                    MatchBody::Expr(e) => infer_expr_type(e, ctx),
                    MatchBody::Block(_) => Type::Dynamic,
                }
            } else {
                Type::Dynamic
            }
        }
        ExprKind::Object { properties } => infer_object(properties, ctx),
        ExprKind::Range { .. } => Type::intrinsic(varn_core::TypeTag::Range),
        ExprKind::Pipeline { right, .. } => infer_expr_type(right, ctx),
        _ => Type::Dynamic,
    }
}

fn infer_member(
    object: &Expr,
    property: &Expr,
    computed: bool,
    ctx: Option<&dyn crate::types::TypeContext>,
) -> Type {
    let obj_ty = infer_expr_type(object, ctx);
    if computed {
        return match &obj_ty.0 {
            TypeKind::Array(inner) => (**inner).clone(),
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => Type::Str,
            TypeKind::Named(name, _) if name.as_ref() == "str" => Type::Str,
            _ => Type::Dynamic,
        };
    }
    let prop_name = match &property.kind {
        ExprKind::Identifier { name } => name,
        _ => return Type::Dynamic,
    };

    if let Some(ctx) = ctx {
        match &obj_ty.0 {
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                if let Some(variants) = ctx.get_enum_members(name.as_ref(), origin.as_deref()) {
                    if prop_name.as_ref() == "rawValue" || prop_name.as_ref() == "__tag" {
                        return Type::Int;
                    }
                    if prop_name.as_ref() == "name" || prop_name.as_ref() == "__variant_name__" {
                        return Type::Str;
                    }
                    let mut found_tys = Vec::new();
                    for v in &variants {
                        if let TypeKind::Fn(ft) = &v.ty.0 {
                            if let Some(p) = ft.params.iter().find(|p| {
                                p.name
                                    .as_ref()
                                    .map_or(false, |pn| pn.as_ref() == prop_name.as_ref())
                            }) {
                                found_tys.push(p.ty.clone());
                            }
                        }
                    }
                    if !found_tys.is_empty() {
                        return Type::union(found_tys);
                    }
                }
                if let Some(members) = ctx
                    .get_class_members(name.as_ref(), origin.as_deref())
                    .or_else(|| ctx.get_interface_members(name.as_ref(), origin.as_deref()))
                    .or_else(|| ctx.get_namespace_members(name.as_ref(), origin.as_deref()))
                    .or_else(|| ctx.get_enum_members(name.as_ref(), origin.as_deref()))
                {
                    if let Some(m) = members
                        .iter()
                        .find(|m| m.name.as_ref() == prop_name.as_ref())
                    {
                        return m.ty.clone();
                    }
                }
            }
            TypeKind::Object(members) => {
                for m in members {
                    if let Some(ty) = infer_object_member_type(m, prop_name) {
                        return ty;
                    }
                }
            }
            TypeKind::Array(inner) => {
                if prop_name.as_ref() == "length" {
                    return Type::Int;
                }
                if prop_name.as_ref() == "push" {
                    return Type::fn_(FunctionType {
                        params: vec![crate::types::FunctionParam {
                            name: Some(Rc::from("item")),
                            ty: (**inner).clone(),
                            optional: false,
                            is_rest: false,
                        }],
                        return_type: Box::new(Type::Int),
                        is_arrow: false,
                        type_params: vec![],
                    });
                }
            }
            _ => {}
        }
    }
    Type::Dynamic
}

fn infer_binary(
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
    ctx: Option<&dyn crate::types::TypeContext>,
) -> Type {
    match op {
        BinaryOp::Add => {
            let l = infer_expr_type(left, ctx);
            let r = infer_expr_type(right, ctx);
            match (&l.0, &r.0) {
                (&TypeKind::Intrinsic(varn_core::TypeTag::Str), _)
                | (_, &TypeKind::Intrinsic(varn_core::TypeTag::Str)) => Type::Str,
                (&TypeKind::Intrinsic(varn_core::TypeTag::Float), _)
                | (_, &TypeKind::Intrinsic(varn_core::TypeTag::Float)) => Type::Float,
                (
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                ) => Type::Int,
                _ => Type::Dynamic,
            }
        }
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
            let l = infer_expr_type(left, ctx);
            let r = infer_expr_type(right, ctx);
            match (&l.0, &r.0) {
                (&TypeKind::Intrinsic(varn_core::TypeTag::Float), _)
                | (_, &TypeKind::Intrinsic(varn_core::TypeTag::Float)) => Type::Float,
                (
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                ) => Type::Int,
                _ => Type::Dynamic,
            }
        }
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::LtEq
        | BinaryOp::GtEq
        | BinaryOp::Instanceof
        | BinaryOp::In => Type::Bool,
        BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::UShr => {
            let l = infer_expr_type(left, ctx);
            let r = infer_expr_type(right, ctx);
            match (&l.0, &r.0) {
                (
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                    &TypeKind::Intrinsic(varn_core::TypeTag::Int),
                ) => Type::Int,
                _ => Type::Dynamic,
            }
        }
    }
}

fn infer_new(
    callee: &Expr,
    type_args: &[varn_core::ast::TypeNode],
    ctx: Option<&dyn crate::types::TypeContext>,
) -> Type {
    if let ExprKind::Identifier { name } = &callee.kind {
        if type_args.is_empty() {
            return Type::named_with_origin(
                name.to_string(),
                ctx.and_then(|c| c.source_file()).map(|s| s.to_owned()),
            );
        }
        let args = type_args
            .iter()
            .map(|m| resolve_type_node(m, ctx))
            .collect();
        return Type::generic_with_origin(
            name.to_string(),
            args,
            ctx.and_then(|c| c.source_file()).map(|s| s.to_owned()),
        );
    }
    if let ExprKind::Member { .. } = &callee.kind {
        let callee_ty = infer_expr_type(callee, ctx);
        match &callee_ty.0 {
            TypeKind::Named(name, origin) => {
                return Type::named_with_origin(
                    name.to_string(),
                    origin.as_ref().map(|s| s.to_string()),
                );
            }
            TypeKind::Generic(name, args, origin) => {
                return Type::generic_with_origin(
                    name.to_string(),
                    args.clone(),
                    origin.as_ref().map(|s| s.to_string()),
                );
            }
            _ => {}
        }
    }
    Type::Dynamic
}

fn infer_object(properties: &[ObjectProp], ctx: Option<&dyn crate::types::TypeContext>) -> Type {
    let mut members = Vec::new();
    for p in properties {
        match p {
            ObjectProp::Property { key, value, .. } => {
                if matches!(key, PropKey::Computed(_)) {
                    let val_ty = infer_expr_type(value, ctx);
                    members.push(ObjectTypeMember::Index {
                        param_name: Rc::from("_key"),
                        key_ty: Box::new(Type::Str),
                        value_ty: Box::new(val_ty),
                    });
                    continue;
                }
                let name = match key {
                    PropKey::Identifier(n) | PropKey::Str(n) => Rc::from(n.as_str()),
                    _ => continue,
                };
                let ty = infer_expr_type(value, ctx);
                if let TypeKind::Fn(ft) = &ty.0 {
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: ft.params.clone(),
                        return_type: ft.return_type.clone(),
                        optional: false,
                        is_arrow: ft.is_arrow,
                    });
                } else {
                    members.push(ObjectTypeMember::Property {
                        name,
                        ty,
                        optional: false,
                        readonly: false,
                    });
                }
            }
            ObjectProp::Method {
                key,
                params,
                return_type,
                ..
            } => {
                let name = match key {
                    PropKey::Identifier(n) | PropKey::Str(n) => Rc::from(n.as_str()),
                    _ => continue,
                };
                let ps = build_method_params(params, ctx);
                let ret = return_type
                    .as_ref()
                    .map(|m| resolve_type_node(m, ctx))
                    .unwrap_or(Type::Dynamic);
                members.push(ObjectTypeMember::Method {
                    name,
                    params: ps,
                    return_type: Box::new(ret),
                    optional: false,
                    is_arrow: false,
                });
            }
            ObjectProp::Spread { argument, .. } => {
                let spread_ty = infer_expr_type(argument, ctx);
                if let TypeKind::Object(spread_members) = spread_ty.0 {
                    members.extend(spread_members);
                }
            }
            _ => {}
        }
    }
    Type::object(members)
}
