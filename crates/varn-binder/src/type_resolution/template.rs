use std::sync::Arc;
use varn_core::ast::TypeNode;
use varn_sem::types::{CheckerTyTable, Type, TypeContext};

pub(super) fn collect_string_literals(_ty: &Type, _table: &CheckerTyTable) -> Vec<Arc<str>> {
    vec![]
}

pub(super) fn resolve_template_literal_type(
    _parts: &[TypeNode],
    _ctx: Option<&dyn TypeContext>,
    _table: &mut CheckerTyTable,
) -> Type {
    Type::Str
}
