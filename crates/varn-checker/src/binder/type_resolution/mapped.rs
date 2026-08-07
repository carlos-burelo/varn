use crate::types::{ObjectTypeMember, Type, TypeContext};
use std::rc::Rc;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

use super::contexts::{is_member_optional, MappedContext};
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
) -> Type {
    let keys = collect_string_literals(&source);
    if keys.is_empty() {
        use varn_core::TypeTag;
        let key_ty = match &source.0 {
            TypeKind::Intrinsic(TypeTag::Str) => Some(Type::Str),
            TypeKind::Intrinsic(TypeTag::Int) => Some(Type::Int),
            _ => None,
        };
        if let Some(key_ty) = key_ty {
            let mapped_ctx = MappedContext {
                inner: ctx,
                key_var: key_var.to_owned(),
                key_value: key_ty.clone(),
            };
            let value_ty = resolve_type_node(value_node, Some(&mapped_ctx));
            return Type::object(vec![ObjectTypeMember::Index {
                param_name: Rc::from(key_var),
                key_ty: Box::new(key_ty),
                value_ty: Box::new(value_ty),
            }]);
        }
        return Type::Dynamic;
    }
    let members: Vec<ObjectTypeMember> = keys
        .into_iter()
        .map(|key| {
            let mapped_ctx = MappedContext {
                inner: ctx,
                key_var: key_var.to_owned(),
                key_value: Type::Str,
            };
            let value_ty = resolve_type_node(value_node, Some(&mapped_ctx));
            let member_optional = if optional {
                true
            } else if let Some(ref obj) = source_obj {
                is_member_optional(obj, key.as_ref(), ctx)
            } else {
                false
            };
            ObjectTypeMember::Property {
                name: key,
                ty: value_ty,
                optional: member_optional,
                readonly,
            }
        })
        .collect();
    Type::object(members)
}
