use crate::types::{CheckerTyTable, Type, TypeContext};
use varn_core::TypeKind;

use super::contexts::AliasSubstitutionContext;
use super::resolve_type_node;


pub(super) fn try_stdlib_generic_alias(
    name: &str,
    args: &[Type],
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    let bind_rc = ctx?.resolver()?.stdlib_bind("core:types/aliases")?;

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

pub fn resolve_primitive(
    name: &str,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    if let Some(kind) = TypeKind::of_lang_name(name) {
        return Type::resolved(table.intern(kind));
    }
    let name_atom = table.intern_name(name);
    let origin = ctx
        .and_then(|c| c.source_file())
        .map(|s| table.intern_name(s));
    Type::named_with_origin_atom(name_atom, origin, table)
}
