use crate::types::{CheckerTyTable, Type, TypeContext};
use vn_core::TypeKind;

use super::names::resolve_atom_name;

pub(super) fn resolve_literal_type(
    lit: vn_core::TypeLiteral<vn_core::Atom>,
    ctx: Option<&dyn TypeContext>,
    interner: &vn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    let l = match lit {
        vn_core::TypeLiteral::Str(a) => {
            let text = resolve_atom_name(a, ctx, interner);
            if !text.is_empty() {
                table.intern_name(&text);
            }
            vn_core::TypeLiteral::Str(a)
        }
        other @ vn_core::TypeLiteral::Int(_)
        | other @ vn_core::TypeLiteral::Bool(_)
        | other @ vn_core::TypeLiteral::Char(_) => other,
    };
    Type::resolved(table.intern(TypeKind::Literal(l)))
}
