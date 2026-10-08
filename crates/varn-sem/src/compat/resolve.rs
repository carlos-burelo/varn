use crate::bind::BindView;
use crate::types::Type;

pub(super) fn ctx_interner<'a>(bind: Option<&'a BindView>) -> Option<&'a varn_core::AtomInterner> {
    bind.map(|b| &b.bind.interner)
}

pub(super) fn resolve_atom(bind: Option<&BindView>, atom: varn_core::Atom) -> Option<String> {
    let b = bind?;
    b.bind
        .interner
        .try_resolve(atom)
        .or_else(|| b.bind.ty_table.name(atom))
        .map(|s| s.to_string())
}

pub(super) fn is_intrinsic(bind: Option<&BindView>, atom: varn_core::Atom, name: &str) -> bool {
    ctx_interner(bind).is_some_and(|i| i.get(name) == Some(atom))
}

pub(super) fn m_ty(m: &crate::types::ClassMemberInfo) -> Type {
    m.ty
}
