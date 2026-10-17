use crate::bind::BindView;
use crate::types::Type;

pub(super) fn resolve_atom(bind: Option<&BindView>, atom: varn_core::Atom) -> Option<String> {
    let b = bind?;
    b.bind
        .interner
        .try_resolve(atom)
        .or_else(|| b.bind.ty_table.name(atom))
        .map(|s| s.to_string())
}

pub(super) fn m_ty(m: &crate::types::ClassMemberInfo) -> Type {
    m.ty
}
