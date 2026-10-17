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
    from: &CheckerTyTable,
    from_source_file: &str,
    to: &mut CheckerTyTable,
) -> Type {
    let ty = rehome_ty(ty, from, to);
    if matches!(
        to.get(ty.0),
        varn_core::TypeKind::Named(_, None) | varn_core::TypeKind::Generic(_, _, None)
    ) {
        let origin = to.intern_name(from_source_file);
        return ty.with_origin(origin, to);
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
