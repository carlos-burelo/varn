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
            let atom = ctx
                .and_then(|c| c.resolver())
                .map_or(a, |r| r.intern(&text));
            varn_core::TypeLiteral::Str(atom)
        }
        other => other,
    };
    Type(table.intern(TypeKind::Literal(l)), false)
}
