use std::sync::Arc;
use crate::types::TypeContext;

pub(super) fn resolve_atom_name(
    atom: varn_core::Atom,
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
) -> Arc<str> {
    if let Some(s) = interner.try_resolve(atom) {
        return Arc::from(s);
    }
    ctx.and_then(|c| c.resolver())
        .and_then(|r| {
            r.interner_snapshot()
                .try_resolve(atom)
                .map(|s| s.to_owned())
        })
        .map(Arc::from)
        .unwrap_or_default()
}
