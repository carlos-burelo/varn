use super::expr::infer_expr_type;
use super::signature::infer_object_member_type;
use crate::types::{CheckerTyTable, Type};
use std::sync::Arc;
use vn_core::ast::{AstArena, ExprId};
use vn_core::TypeKind;

pub(crate) fn ctx_resolve_text(
    ctx: Option<&dyn crate::types::TypeContext>,
    atom: vn_core::Atom,
) -> Option<String> {
    ctx?.atom_text(atom)
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
    crate::rehome::rehome_ty_with_origin(ty, &b, table)
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
            TypeKind::Array(inner) => Type::resolved(inner),
            TypeKind::Primitive(vn_core::LangPrimitive::Str) => Type::Str,
            TypeKind::Generic(..) if obj_ty.sequence_element(table).is_some() => {
                obj_ty.builtin_arg(0, table)
            }
            TypeKind::Generic(..) if obj_ty.keyed_collection(table).is_some() => {
                obj_ty.builtin_arg(1, table)
            }
            TypeKind::Record(_) | TypeKind::Interface(_) => Type::Dynamic,
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => Type::Dynamic,
        };
    }
    let prop_name_atom = match &arena.expr(property).kind {
        vn_core::ast::ExprKind::Identifier { name } => *name,
        vn_core::ast::ExprKind::IntLiteral { .. }
        | vn_core::ast::ExprKind::FloatLiteral { .. }
        | vn_core::ast::ExprKind::BigIntLiteral { .. }
        | vn_core::ast::ExprKind::DecimalLiteral { .. }
        | vn_core::ast::ExprKind::StrLiteral { .. }
        | vn_core::ast::ExprKind::CharLiteral { .. }
        | vn_core::ast::ExprKind::BoolLiteral { .. }
        | vn_core::ast::ExprKind::NullLiteral
        | vn_core::ast::ExprKind::Template { .. }
        | vn_core::ast::ExprKind::TaggedTemplate { .. }
        | vn_core::ast::ExprKind::Missing
        | vn_core::ast::ExprKind::This
        | vn_core::ast::ExprKind::ImportMeta(_)
        | vn_core::ast::ExprKind::Super
        | vn_core::ast::ExprKind::Array { .. }
        | vn_core::ast::ExprKind::Object { .. }
        | vn_core::ast::ExprKind::Tuple { .. }
        | vn_core::ast::ExprKind::Unary { .. }
        | vn_core::ast::ExprKind::Update { .. }
        | vn_core::ast::ExprKind::Binary { .. }
        | vn_core::ast::ExprKind::Logical { .. }
        | vn_core::ast::ExprKind::Assign { .. }
        | vn_core::ast::ExprKind::Conditional { .. }
        | vn_core::ast::ExprKind::Member { .. }
        | vn_core::ast::ExprKind::Call { .. }
        | vn_core::ast::ExprKind::New { .. }
        | vn_core::ast::ExprKind::Function { .. }
        | vn_core::ast::ExprKind::Arrow { .. }
        | vn_core::ast::ExprKind::Sequence { .. }
        | vn_core::ast::ExprKind::Paren { .. }
        | vn_core::ast::ExprKind::Await { .. }
        | vn_core::ast::ExprKind::Spawn { .. }
        | vn_core::ast::ExprKind::Yield { .. }
        | vn_core::ast::ExprKind::Spread { .. }
        | vn_core::ast::ExprKind::Pipeline { .. }
        | vn_core::ast::ExprKind::Range { .. }
        | vn_core::ast::ExprKind::NonNull { .. }
        | vn_core::ast::ExprKind::Try { .. }
        | vn_core::ast::ExprKind::As { .. }
        | vn_core::ast::ExprKind::Satisfies { .. }
        | vn_core::ast::ExprKind::ClassExpr { .. }
        | vn_core::ast::ExprKind::Match { .. }
        | vn_core::ast::ExprKind::Is { .. }
        | vn_core::ast::ExprKind::With { .. }
        | vn_core::ast::ExprKind::MetaAccess { .. } => return Type::Dynamic,
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
                    if prop_name == vn_core::MemberKey::RawValue.as_str()
                        || prop_name == vn_core::MemberKey::Tag.as_str()
                    {
                        return Type::Int;
                    }
                    if prop_name == vn_core::MemberKey::Name.as_str()
                        || prop_name == vn_core::MemberKey::VariantName.as_str()
                    {
                        return Type::Str;
                    }
                    let mut found_tys = Vec::new();
                    for v in &variants {
                        let vty =
                            reintern_member_type(Some(ctx), origin_str.as_deref(), v.ty, table);
                        if let TypeKind::Fn(fid) = table.get(vty.0)
                            && let Some(p) = table.get_function(fid).params.iter().find(|p| {
                                p.name.as_ref().is_some_and(|pn| pn.as_ref() == prop_name)
                            })
                        {
                            found_tys.push(Type::resolved(p.ty));
                        }
                    }
                    if !found_tys.is_empty() {
                        return Type::union(found_tys, table);
                    }
                }
                let origin = origin_str.as_deref();
                let found = [
                    ctx.get_class_members(&name_str, origin),
                    ctx.get_interface_members(&name_str, origin),
                    ctx.get_namespace_members(&name_str, origin),
                    ctx.get_enum_members(&name_str, origin),
                ]
                .into_iter()
                .flatten()
                .find_map(|members| {
                    members
                        .into_iter()
                        .find(|m| m.name.as_ref() == prop_name)
                        .map(|m| m.ty)
                });
                if let Some(ty) = found {
                    return reintern_member_type(Some(ctx), origin, ty, table);
                }
            }
            TypeKind::Record(mid) | TypeKind::Interface(mid) => {
                for m in table.get_object_members(mid).to_vec() {
                    if let Some(ty) = infer_object_member_type(&m, prop_name, table) {
                        return ty;
                    }
                }
            }
            TypeKind::Array(inner) => {
                if prop_name == vn_core::MemberKey::Length.as_str() {
                    return Type::Int;
                }
                if prop_name == vn_core::MemberKey::Push.as_str() {
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
                            type_param_defaults: Vec::new(),
                        },
                        table,
                    );
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
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
