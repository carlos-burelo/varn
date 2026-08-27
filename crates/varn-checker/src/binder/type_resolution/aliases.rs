use crate::types::{ObjectTypeMember, Type, TypeContext};
use std::rc::Rc;
use varn_core::{IntrinsicType, TypeKind, TypeTag};

use super::contexts::AliasSubstitutionContext;
use super::resolve_type_node;

/// Expand `name<args>` when `name` is a generic alias declared in `core:types`.
pub(super) fn try_stdlib_generic_alias(
    name: &str,
    args: &[Type],
    ctx: Option<&dyn TypeContext>,
) -> Option<Type> {
    if name == "Record" && args.len() == 2 {
        let key_ty = &args[0];
        let value_ty = &args[1];
        if matches!(key_ty.0, TypeKind::Intrinsic(TypeTag::Str)) {
            return Some(Type::object(vec![ObjectTypeMember::Index {
                param_name: Rc::from("key"),
                key_ty: Box::new(key_ty.clone()),
                value_ty: Box::new(value_ty.clone()),
            }]));
        }
    }

    let bind_rc = ctx?.resolver()?.stdlib_bind("core:types")?;

    let (params, alias_node) = bind_rc.get_alias_node_local(name)?;
    if params.is_empty() || params.len() != args.len() {
        return None;
    }
    let alias_ctx = AliasSubstitutionContext {
        inner: ctx,
        params,
        args: args.to_vec(),
    };
    Some(resolve_type_node(&alias_node, Some(&alias_ctx)))
}

pub(super) fn is_primitive_type(ty: &Type) -> bool {
    matches!(
        &ty.0,
        TypeKind::Intrinsic(tag) if tag.is_primitive()
    )
}

pub fn resolve_primitive(name: &str, ctx: Option<&dyn TypeContext>) -> Type {
    use varn_core::TypeKind as K;
    if let Some(it) = IntrinsicType::from_str(name) {
        return Type(K::Intrinsic(it.0), false);
    }
    Type::named_with_origin(
        name.to_owned(),
        ctx.and_then(|c| c.source_file()).map(|s| s.to_owned()),
    )
}
