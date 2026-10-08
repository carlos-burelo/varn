use varn_core::ast::TypeNode;
use varn_sem::types::{CheckerTyTable, Type, TypeContext};

use super::resolve_type_node;

pub(super) fn resolve_array_type(
    inner: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let inner_ty = resolve_type_node(inner, ctx, table);
    Type::array(inner_ty, table)
}

pub(super) fn resolve_union_type(
    members: &[TypeNode],
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let resolved: Vec<Type> = members
        .iter()
        .map(|m| resolve_type_node(m, ctx, table))
        .collect();
    Type::union(resolved, table)
}
