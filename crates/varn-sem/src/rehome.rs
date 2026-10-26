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
    if let varn_core::TypeKind::Named(name, None) | varn_core::TypeKind::Generic(name, _, None) =
        to.get(ty.0)
    {
        let nominal = from
            .ty_table
            .names()
            .try_resolve(name)
            .is_some_and(|name| from.classify_decl(name).is_some());
        if nominal {
            let origin = to.intern_name(from.source_file.as_ref());
            return ty.with_origin(origin, to);
        }
    }
    ty
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
