use super::expr::infer_expr_type;
use crate::resolve::resolve_type_node;
use crate::types::numeric_ops::{adopt_literal_operands, numeric_binary_type};
use crate::types::{CheckerTyTable, ObjectTypeMember, Type};
use std::sync::Arc;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{AstArena, ExprId, ObjectProp, PropKey};
use varn_core::TypeKind;

pub(crate) fn infer_binary(
    op: &BinaryOp,
    left: ExprId,
    right: ExprId,
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    match op {
        BinaryOp::Add => {
            let l = infer_expr_type(left, arena, ctx, table).apparent(table);
            let r = infer_expr_type(right, arena, ctx, table).apparent(table);
            match (table.get(l.0), table.get(r.0)) {
                (TypeKind::Primitive(varn_core::LangPrimitive::Str), _)
                | (_, TypeKind::Primitive(varn_core::LangPrimitive::Str)) => Type::Str,
                _ => {
                    let (l, r) = adopt_literal_operands(arena, left, right, l, r, table);
                    numeric_binary_type(&l, &r, table).unwrap_or(Type::Dynamic)
                }
            }
        }
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
            let l = infer_expr_type(left, arena, ctx, table).apparent(table);
            let r = infer_expr_type(right, arena, ctx, table).apparent(table);
            let (l, r) = adopt_literal_operands(arena, left, right, l, r, table);
            numeric_binary_type(&l, &r, table).unwrap_or(Type::Dynamic)
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
            let l = infer_expr_type(left, arena, ctx, table).apparent(table);
            let r = infer_expr_type(right, arena, ctx, table).apparent(table);
            match (table.get(l.0), table.get(r.0)) {
                (
                    TypeKind::Primitive(varn_core::LangPrimitive::Int),
                    TypeKind::Primitive(varn_core::LangPrimitive::Int),
                ) => Type::Int,
                _ => Type::Dynamic,
            }
        }
    }
}

pub fn infer_new(
    callee: ExprId,
    type_args: &[varn_core::ast::TypeNode],
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    use varn_core::ast::ExprKind;
    if let ExprKind::Identifier { name } = &arena.expr(callee).kind {
        let name_str = ctx
            .and_then(|c| c.interner())
            .map(|i| i.resolve(*name).to_owned())
            .unwrap_or_default();
        let origin = crate::resolve::nominal_origin(&name_str, ctx, table);
        if type_args.is_empty() {
            if varn_core::BuiltinType::identify(*name, origin) == Some(varn_core::BuiltinType::Map)
            {
                return Type::generic_atom(
                    *name,
                    vec![Type::Dynamic, Type::Dynamic],
                    origin,
                    table,
                );
            }
            return Type::named_with_origin_atom(*name, origin, table);
        }
        let args = type_args
            .iter()
            .map(|m| resolve_type_node(m, ctx, table))
            .collect();
        return Type::generic_atom(*name, args, origin, table);
    }
    if let ExprKind::Member { .. } = &arena.expr(callee).kind {
        let callee_ty = infer_expr_type(callee, arena, ctx, table);
        match table.get(callee_ty.0) {
            TypeKind::Named(name, origin) => {
                return Type::named_with_origin_atom(name, origin, table);
            }
            TypeKind::Generic(name, args, origin) => {
                let arg_ids = table.get_list(args).to_vec();
                let arg_tys: Vec<Type> = arg_ids.into_iter().map(Type::resolved).collect();
                return Type::generic_atom(name, arg_tys, origin, table);
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => {}
        }
    }
    Type::Dynamic
}

pub(crate) fn infer_object(
    properties: &[ObjectProp],
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    use super::signature::build_method_params;
    let mut members = Vec::new();
    for p in properties {
        match p {
            ObjectProp::Property { key, value, .. } => {
                let value = *value;
                if matches!(key, PropKey::Computed(_)) {
                    let val_ty = infer_expr_type(value, arena, ctx, table);
                    members.push(ObjectTypeMember::Index {
                        param_name: Arc::from("_key"),
                        key_ty: Type::Str.0,
                        value_ty: val_ty.0,
                    });
                    continue;
                }
                let name = match key {
                    PropKey::Identifier(n) | PropKey::Str(n) => Arc::from(n.as_str()),
                    PropKey::Int(_) | PropKey::Computed(_) => continue,
                };
                let ty = infer_expr_type(value, arena, ctx, table);
                if let TypeKind::Fn(fid) = table.get(ty.0) {
                    let ft = table.get_function(fid).clone();
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: ft.params.clone(),
                        return_type: ft.return_type,
                        optional: false,
                        is_arrow: ft.is_arrow,
                        type_params: ft.type_params.clone(),
                    });
                } else {
                    members.push(ObjectTypeMember::Property {
                        name,
                        ty: ty.0,
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
                    PropKey::Identifier(n) | PropKey::Str(n) => Arc::from(n.as_str()),
                    PropKey::Int(_) | PropKey::Computed(_) => continue,
                };
                let ps = build_method_params(params, ctx, table);
                let ret = return_type
                    .as_ref()
                    .map(|m| resolve_type_node(m, ctx, table))
                    .unwrap_or(Type::Dynamic);
                members.push(ObjectTypeMember::Method {
                    name,
                    params: ps,
                    return_type: ret.0,
                    optional: false,
                    is_arrow: false,
                    type_params: Vec::new(),
                });
            }
            ObjectProp::Spread { argument, .. } => {
                let spread_ty = infer_expr_type(*argument, arena, ctx, table);
                if let TypeKind::Object(mid) = table.get(spread_ty.0) {
                    members.extend(table.get_object_members(mid).to_vec());
                }
            }
            ObjectProp::Getter { .. } | ObjectProp::Setter { .. } => {}
        }
    }
    Type::object(members, table)
}
