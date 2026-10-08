mod aliases;
mod compound;
mod conditional;
mod contexts;
mod functions;
mod intersections;
mod keyed_access;
mod mapped;
mod names;
mod nominal;
mod objects;
mod scalar;
mod template;

use varn_core::ast::TypeNode;
use varn_core::TypeKind;
use varn_sem::types::{CheckerTyTable, Type, TypeContext};

use conditional::resolve_conditional;
use keyed_access::{resolve_indexed_access, resolve_keyof};
use mapped::resolve_mapped_type;
use template::resolve_template_literal_type;

pub use aliases::resolve_primitive;

pub fn resolve_type_node(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let default_interner = varn_core::AtomInterner::new();
    let interner = ctx.and_then(|c| c.interner()).unwrap_or(&default_interner);
    match &node.kind {
        TypeKind::Primitive(varn_core::LangPrimitive::Int) => Type::Int,
        TypeKind::Primitive(varn_core::LangPrimitive::Float) => Type::Float,
        TypeKind::Primitive(varn_core::LangPrimitive::Decimal) => Type::Decimal,
        TypeKind::Primitive(varn_core::LangPrimitive::BigInt) => Type::BigInt,
        TypeKind::Primitive(varn_core::LangPrimitive::Str) => Type::Str,
        TypeKind::Primitive(varn_core::LangPrimitive::Char) => Type::Char,
        TypeKind::Primitive(varn_core::LangPrimitive::Bool) => Type::Bool,
        TypeKind::Primitive(varn_core::LangPrimitive::Void) => Type::Void,
        TypeKind::Primitive(varn_core::LangPrimitive::Null) => Type::Null,
        TypeKind::Primitive(varn_core::LangPrimitive::Never) => Type::Never,
        TypeKind::Primitive(varn_core::LangPrimitive::Dynamic) => Type::Dynamic,
        TypeKind::Builtin(b) => Type::builtin(*b, table),
        TypeKind::This => Type::This,
        TypeKind::Literal(l) => scalar::resolve_literal_type(*l, ctx, interner, table),
        TypeKind::Array(inner) => compound::resolve_array_type(inner, ctx, table),
        TypeKind::Union(members) => compound::resolve_union_type(members, ctx, table),
        TypeKind::Generic(name, args, _) => {
            nominal::resolve_generic_type(*name, args, ctx, interner, table)
        }
        TypeKind::Named(name, _) => nominal::resolve_named_type(*name, ctx, interner, table),
        TypeKind::Fn((params, ret)) => {
            functions::resolve_fn_type(params, ret, ctx, interner, table)
        }
        TypeKind::Object(members) => objects::resolve_object_type(members, ctx, interner, table),
        TypeKind::TemplateLiteral(parts) => resolve_template_literal_type(parts, ctx, table),
        TypeKind::Typeof(expr) => match ctx.and_then(|c| c.ast_arena()) {
            Some(arena) => crate::infer_expr_type(*expr, arena, ctx, table),
            None => Type::Dynamic,
        },
        TypeKind::Intersection(members) => {
            intersections::resolve_intersection_type(members, ctx, table)
        }
        TypeKind::KeyOf(inner) => {
            let resolved = resolve_type_node(inner, ctx, table);
            resolve_keyof(resolved, ctx, table)
        }
        TypeKind::IndexedAccess { object, index } => {
            let obj = resolve_type_node(object, ctx, table);
            let idx = resolve_type_node(index, ctx, table);
            resolve_indexed_access(obj, idx, ctx, table)
        }
        TypeKind::Mapped {
            key_var,
            source,
            value,
            optional,
            readonly,
        } => resolve_mapped_type(
            *key_var, source, value, *optional, *readonly, ctx, interner, table,
        ),
        TypeKind::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let check_ty = resolve_type_node(check, ctx, table);
            resolve_conditional(check, &check_ty, extends, true_type, false_type, ctx, table)
        }
        TypeKind::Infer(_) => Type::Error,
        TypeKind::Tuple(elements) => {
            let ids: Vec<_> = elements
                .iter()
                .map(|e| resolve_type_node(e, ctx, table).0)
                .collect();
            let list = table.intern_list(&ids);
            Type::resolved(table.intern(TypeKind::Tuple(list)))
        }
        TypeKind::EnumVariant {
            enum_name,
            variant_name,
            type_args,
            payload_ty,
        } => {
            let args: Vec<_> = type_args
                .iter()
                .map(|a| resolve_type_node(a, ctx, table).0)
                .collect();
            let type_args = table.intern_list(&args);
            let payload_ty = resolve_type_node(payload_ty, ctx, table).0;
            Type::resolved(table.intern(TypeKind::EnumVariant {
                enum_name: *enum_name,
                variant_name: *variant_name,
                type_args,
                payload_ty,
            }))
        }
        TypeKind::TypePredicate {
            parameter_name,
            target_type,
        } => {
            let target = resolve_type_node(target_type, ctx, table);
            let interned = table.intern(TypeKind::TypePredicate {
                parameter_name: *parameter_name,
                target_type: target.0,
            });
            Type::resolved(interned)
        }
    }
}
