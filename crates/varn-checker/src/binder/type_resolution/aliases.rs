use crate::types::{Type, TypeContext};
use varn_core::{IntrinsicType, TypeKind};

use super::contexts::AliasSubstitutionContext;
use super::resolve_type_node;

/// Expand `name<args>` when `name` is a generic alias declared in `std:types`.
///
/// The bind comes from the context's resolver, which memoizes it and refuses
/// to re-enter a module it is already binding — `std:types` resolves type
/// nodes while binding, and so asks for itself. That guard used to live here
/// as a thread-local flag beside a thread-local cache of the module.
pub(super) fn try_stdlib_generic_alias(
    name: &str,
    args: &[Type],
    ctx: Option<&dyn TypeContext>,
) -> Option<Type> {
    let bind_rc = ctx?.resolver()?.stdlib_bind("std:types")?;

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
