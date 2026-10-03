use super::inference_utils::infer_object_member_type;
use super::type_infer_expr::infer_expr_type;
use crate::types::{CheckerTyTable, Type};
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprId};
use varn_core::TypeKind;

pub(crate) fn ctx_resolve_text(
    ctx: Option<&dyn crate::types::TypeContext>,
    atom: varn_core::Atom,
) -> Option<String> {
    let ctx = ctx?;
    let interner = ctx.interner()?;
    let s = interner.try_resolve(atom)?;
    Some(s.to_string())
}

pub(crate) fn reintern_member_type(
    ctx: Option<&dyn crate::types::TypeContext>,
    origin: Option<&str>,
    ty: Type,
    table: &mut CheckerTyTable,
) -> Type {
    let Some(ctx) = ctx else { return ty };
    if table.contains(ty.0) {
        return ty;
    }
    let Some(origin) = origin else { return ty };
    if ctx.source_file().is_some_and(|s| s == origin) {
        return ty;
    }
    let Some(resolver) = ctx.resolver() else {
        return ty;
    };
    let Some(b) = resolver
        .stdlib_bind(origin)
        .or_else(|| resolver.module_bind(origin))
    else {
        return ty;
    };
    if !b.ty_table.contains(ty.0) {
        return ty;
    }
    table.absorb(&b.ty_table);
    if matches!(
        table.get(ty.0),
        varn_core::TypeKind::Named(_, None) | varn_core::TypeKind::Generic(_, _, None)
    ) {
        let origin = table.intern_name(b.source_file.as_ref());
        return ty.with_origin(origin, table);
    }
    ty
}

pub(crate) fn infer_member(
    object: ExprId,
    property: ExprId,
    computed: bool,
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let obj_ty = infer_expr_type(object, arena, ctx, table);
    if computed {
        return match table.get(obj_ty.0) {
            TypeKind::Array(inner) => Type(inner, false),
            TypeKind::Primitive(varn_core::LangPrimitive::Str) => Type::Str,
            TypeKind::Named(name, _)
                if ctx
                    .and_then(|c| c.interner())
                    .is_some_and(|i| i.get(varn_core::LangPrimitive::Str.name()) == Some(name)) =>
            {
                Type::Str
            }
            TypeKind::Generic(name, args, _)
                if ctx
                    .and_then(|c| c.interner())
                    .is_some_and(|i| i.get(varn_core::BuiltinType::Map.name()) == Some(name)) =>
            {
                let arg_ids = table.get_list(args).to_vec();
                if arg_ids.len() == 2 {
                    Type(arg_ids[1], false)
                } else {
                    Type::Dynamic
                }
            }
            TypeKind::Object(mid) => table
                .get_object_members(mid)
                .iter()
                .find_map(|m| match m {
                    crate::types::ObjectTypeMember::Index { value_ty, .. } => Some(*value_ty),
                    _ => None,
                })
                .map(|id| Type(id, false))
                .unwrap_or(Type::Dynamic),
            _ => Type::Dynamic,
        };
    }
    let prop_name_atom = match &arena.expr(property).kind {
        varn_core::ast::ExprKind::Identifier { name } => *name,
        _ => return Type::Dynamic,
    };

    if let Some(ctx) = ctx {
        let Some(interner) = ctx.interner() else {
            return Type::Dynamic;
        };
        let prop_name = interner.resolve(prop_name_atom);
        match table.get(obj_ty.0) {
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                let Some(name_str) = ctx_resolve_text(Some(ctx), name) else {
                    return Type::Dynamic;
                };
                let origin_str = origin.and_then(|o| ctx_resolve_text(Some(ctx), o));
                if let Some(variants) = ctx.get_enum_members(&name_str, origin_str.as_deref()) {
                    if prop_name == varn_core::MemberKey::RawValue.as_str()
                        || prop_name == varn_core::MemberKey::Tag.as_str()
                    {
                        return Type::Int;
                    }
                    if prop_name == varn_core::MemberKey::Name.as_str()
                        || prop_name == varn_core::MemberKey::VariantName.as_str()
                    {
                        return Type::Str;
                    }
                    let mut found_tys = Vec::new();
                    for v in &variants {
                        let vty =
                            reintern_member_type(Some(ctx), origin_str.as_deref(), v.ty, table);
                        if let TypeKind::Fn(fid) = table.get(vty.0) {
                            if let Some(p) = table.get_function(fid).params.iter().find(|p| {
                                p.name.as_ref().is_some_and(|pn| pn.as_ref() == prop_name)
                            }) {
                                found_tys.push(Type(p.ty, false));
                            }
                        }
                    }
                    if !found_tys.is_empty() {
                        return Type::union(found_tys, table);
                    }
                }
                if let Some(members) = ctx
                    .get_class_members(&name_str, origin_str.as_deref())
                    .or_else(|| ctx.get_interface_members(&name_str, origin_str.as_deref()))
                    .or_else(|| ctx.get_namespace_members(&name_str, origin_str.as_deref()))
                    .or_else(|| ctx.get_enum_members(&name_str, origin_str.as_deref()))
                {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                        return reintern_member_type(Some(ctx), origin_str.as_deref(), m.ty, table);
                    }
                }
            }
            TypeKind::Object(mid) => {
                for m in table.get_object_members(mid).to_vec() {
                    if let Some(ty) = infer_object_member_type(&m, prop_name, table) {
                        return ty;
                    }
                }
            }
            TypeKind::Array(inner) => {
                if prop_name == varn_core::MemberKey::Length.as_str() {
                    return Type::Int;
                }
                if prop_name == varn_core::MemberKey::Push.as_str() {
                    return Type::fn_(
                        crate::types::FunctionType {
                            params: vec![crate::types::FunctionParam {
                                name: Some(Arc::from("item")),
                                ty: inner,
                                optional: false,
                                is_rest: false,
                            }],
                            return_type: Type::Int.0,
                            is_arrow: false,
                            type_params: vec![],
                        },
                        table,
                    );
                }
            }
            _ => {}
        }
    }
    Type::Dynamic
}
