use crate::binder::BindResult;

pub(super) fn resolve_atom_text(
    resolver: &dyn crate::module_resolver::ImportResolver,
    bind: &BindResult,
    atom: varn_core::Atom,
) -> String {
    if let Some(s) = bind.interner.try_resolve(atom) {
        return s.to_owned();
    }
    resolver
        .interner_snapshot()
        .try_resolve(atom)
        .map(str::to_owned)
        .unwrap_or_default()
}
