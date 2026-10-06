use crate::checker::Checker;
use crate::types::{CheckerTyTable, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::TypeKind;

fn is_generic_possible(ty: &Type, table: &CheckerTyTable) -> bool {
    !matches!(
        table.get(ty.0),
        TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This
    )
}

pub(crate) fn map_generics_cached(
    checker: &mut Checker,
    base: &Type,
    mapping: &FxHashMap<Arc<str>, Type>,
) -> Type {
    if mapping.is_empty() || !is_generic_possible(base, &checker.ty_table) {
        return *base;
    }

    let atom_mapping: FxHashMap<varn_core::Atom, Type> = mapping
        .iter()
        .map(|(k, v)| (varn_core::Atom::of(k), *v))
        .collect();

    if let TypeKind::Named(n, _) = checker.ty_table.get(base.0) {
        if let Some(t) = atom_mapping.get(&n) {
            return *t;
        } else {
            return *base;
        }
    }

    let sorted_args: Vec<Type> = {
        let mut pairs: Vec<(&Arc<str>, &Type)> = mapping.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        pairs.into_iter().map(|(_, v)| *v).collect()
    };
    let key = (*base, sorted_args);
    if let Some(cached) = checker.map_generics_cache.get(&key) {
        return *cached;
    }
    let result = base.map_generics(
        &atom_mapping,
        &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
    );
    checker.map_generics_cache.insert(key, result);
    result
}
