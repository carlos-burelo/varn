use crate::types::{CheckerTyTable, Type, TypeContext};
use varn_core::TypeKind;

use super::names::resolve_atom_name;

pub(super) fn resolve_literal_type(
    lit: varn_core::TypeLiteral<varn_core::Atom>,
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    let l = match lit {
        varn_core::TypeLiteral::Str(a) => {
            let text = resolve_atom_name(a, ctx, interner);
            if !text.is_empty() {
                table.intern_name(&text);
            }
            varn_core::TypeLiteral::Str(a)
        }
        other @ varn_core::TypeLiteral::Int(_)
        | other @ varn_core::TypeLiteral::Bool(_)
        | other @ varn_core::TypeLiteral::Char(_) => other,
    };
    Type::resolved(table.intern(TypeKind::Literal(l)))
}
