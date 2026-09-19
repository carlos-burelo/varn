use crate::types::{CheckerTyTable, Type, TypeContext};
use varn_core::{IntrinsicType, TypeKind};

use super::contexts::AliasSubstitutionContext;
use super::resolve_type_node;

/// Expand `name<args>` when `name` is a generic alias declared in `core:types`.
pub(super) fn try_stdlib_generic_alias(
    name: &str,
    args: &[Type],
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Option<Type> {
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
    Some(resolve_type_node(&alias_node, Some(&alias_ctx), table))
}

pub(super) fn is_primitive_type(ty: &Type, table: &CheckerTyTable) -> bool {
    matches!(
        table.get(ty.0),
        TypeKind::Intrinsic(tag) if tag.is_primitive()
    )
}

pub fn resolve_primitive(name: &str, ctx: Option<&dyn TypeContext>, table: &mut CheckerTyTable) -> Type {
    use varn_core::TypeKind as K;
    if let Some(it) = IntrinsicType::from_str(name) {
        return Type(table.intern(K::Intrinsic(it.0)), false);
    }
    // `name`/`source_file` may not already be interned `Atom`s (the latter is
    // stored as a plain `Rc<str>`, never routed through `AtomInterner`), so
    // this mints them through the resolver's shared table rather than the
    // read-only `TypeContext::interner()` accessor — the same sanctioned path
    // `ImportResolver::intern`'s doc comment describes for callers with no
    // mutable interner of their own on hand.
    let resolver = ctx.and_then(|c| c.resolver());
    let name_atom = resolver
        .map(|r| r.intern(name))
        .or_else(|| ctx.and_then(|c| c.interner()).and_then(|i| i.get(name)))
        .unwrap_or_default();
    let origin = ctx
        .and_then(|c| c.source_file())
        .and_then(|s| resolver.map(|r| r.intern(s)));
    Type::named_with_origin_atom(name_atom, origin, table)
}
