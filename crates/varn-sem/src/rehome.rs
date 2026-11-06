use crate::symbol::Symbol;
use crate::types::{CheckerTyTable, Type};

pub fn rehome_ty(ty: Type, from: &CheckerTyTable, to: &mut CheckerTyTable) -> Type {
    if !to.contains(ty.0) {
        to.absorb(from);
    }
    ty
}

pub fn rehome_ty_with_origin(
    ty: Type,
    from: &crate::bind::BindResult,
    to: &mut CheckerTyTable,
) -> Type {
    let ty = rehome_ty(ty, &from.ty_table, to);
    let origin = to.intern_name(from.source_file.as_ref());
    ty.qualify_nominals(
        origin,
        &|name| {
            from.ty_table
                .names()
                .try_resolve(name)
                .is_some_and(|name| from.classify_decl(name).is_some())
        },
        to,
    )
}

pub fn rehome_symbol(sym: &Symbol, from: &CheckerTyTable, to: &mut CheckerTyTable) -> Symbol {
    let mut out = sym.clone();
    if let Some(ty) = out.ty {
        out.ty = Some(rehome_ty(ty, from, to));
    }
    for slot in out
        .type_param_constraints
        .iter_mut()
        .chain(out.type_param_defaults.iter_mut())
        .flatten()
    {
        *slot = rehome_ty(*slot, from, to);
    }
    out
}
