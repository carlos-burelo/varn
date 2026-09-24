use crate::types::{CheckerTyTable, ObjectTypeMember, Type, TypeContext};
use std::sync::Arc;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use super::contexts::{is_member_optional, MappedContext};
use super::keyed_access::collect_type_keys;
use super::resolve_type_node;
use super::template::collect_string_literals;

pub(super) fn resolve_mapped(
    key_var: &str,
    source: Type,
    value_node: &TypeNode,
    optional: bool,
    readonly: bool,
    source_obj: Option<Type>,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let keys = if let Some(obj) = source_obj {
        let k = collect_type_keys(&obj, ctx, table);
        if k.is_empty() {
            collect_string_literals(&source, table)
        } else {
            k
        }
    } else {
        collect_string_literals(&source, table)
    };

    if keys.is_empty() {
        
        let key_ty = match table.get(source.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Str) => Some(Type::Str),
            TypeKind::Primitive(varn_core::LangPrimitive::Int) => Some(Type::Int),
            _ => None,
        };
        if let Some(key_ty) = key_ty {
            let mapped_ctx = MappedContext {
                inner: ctx,
                key_var: key_var.to_owned(),
                key_value: key_ty,
            };
            let value_ty = resolve_type_node(value_node, Some(&mapped_ctx), table);
            let member = ObjectTypeMember::Index {
                param_name: Arc::from(key_var),
                key_ty: key_ty.0,
                value_ty: value_ty.0,
            };
            return Type::object(vec![member], table);
        }
        return Type::Dynamic;
    }

    let members: Vec<ObjectTypeMember> = keys
        .into_iter()
        .map(|key| {
            let key_atom = ctx
                .and_then(|c| c.resolver())
                .map(|r| r.intern(&key))
                .unwrap_or_default();
            let key_value = Type::named_atom(key_atom, table);
            let mapped_ctx = MappedContext {
                inner: ctx,
                key_var: key_var.to_owned(),
                key_value,
            };
            let value_ty = resolve_type_node(value_node, Some(&mapped_ctx), table);
            let member_optional = if optional {
                true
            } else if let Some(ref obj) = source_obj {
                is_member_optional(obj, key.as_ref(), ctx, table)
            } else {
                false
            };
            ObjectTypeMember::Property {
                name: key,
                ty: value_ty.0,
                optional: member_optional,
                readonly,
            }
        })
        .collect();
    Type::object(members, table)
}
