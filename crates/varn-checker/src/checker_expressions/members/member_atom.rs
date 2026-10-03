use crate::binder::BindResult;

pub(super) fn resolve_atom_text(bind: &BindResult, atom: varn_core::Atom) -> String {
    bind.interner
        .try_resolve(atom)
        .or_else(|| bind.ty_table.name(atom))
        .map(str::to_owned)
        .unwrap_or_default()
}
