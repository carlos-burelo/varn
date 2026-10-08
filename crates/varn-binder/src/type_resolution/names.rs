use std::sync::Arc;
use varn_sem::types::TypeContext;

pub(super) fn resolve_atom_name(
    atom: varn_core::Atom,
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
) -> Arc<str> {
    if let Some(s) = interner.try_resolve(atom) {
        return Arc::from(s);
    }
    ctx.and_then(|c| c.atom_text(atom))
        .map(Arc::from)
        .unwrap_or_default()
}
