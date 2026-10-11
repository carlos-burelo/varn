use crate::symbol::Symbol;
use crate::types::{CheckerTyTable, Type};

pub fn canonicalize_lang_named(ty: Type, table: &mut CheckerTyTable) -> Type {
    let (name_atom, origin_atom) = match table.get(ty.0) {
        varn_core::TypeKind::Named(n, o) => (n, o),
        _ => return ty,
    };
    let Some(origin) = origin_atom else {
        return ty;
    };
    let (Some(name_s), Some(origin_s)) = (table.name(name_atom), table.name(origin)) else {
        return ty;
    };
    let (name_s, origin_s) = (name_s.to_string(), origin_s.to_string());
    if let Some(b) = varn_core::BuiltinType::from_str(&name_s) {
        if origin_s == b.core_module() {
            return Type::builtin(b, table);
        }
    }
    if let Some(p) = varn_core::LangPrimitive::from_str(&name_s) {
        if Some(origin_s.as_str()) == p.core_module() {
            return Type::primitive(p, table);
        }
    }
    ty
}

pub fn rehome_ty(ty: Type, from: &CheckerTyTable, to: &mut CheckerTyTable) -> Type {
    if !to.contains(ty.0) {
        to.absorb(from);
    }
    canonicalize_lang_named(ty, to)
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
        let ty = ty.with_origin(origin, to);
        return canonicalize_lang_named(ty, to);
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
