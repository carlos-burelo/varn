use super::resolve::resolve_atom;
use super::scalar::t;
use super::variance::Variance;
use crate::bind::BindView;
use crate::types::InternedTypeKind;
use crate::types::{CheckerTyId, CheckerTyTable, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

fn measured_variance(
    bind: Option<&BindView>,
    name: varn_core::Atom,
    origin: Option<varn_core::Atom>,
) -> Option<Vec<Variance>> {
    let b = bind?;
    let name_str = super::resolve::resolve_atom(bind, name)?;
    let foreign = origin
        .and_then(|o| super::resolve::resolve_atom(bind, o))
        .filter(|o| o.as_str() != b.bind.source_file.as_ref());
    if let Some(o) = foreign {
        let rb = b
            .resolver
            .module_bind(&o)
            .or_else(|| b.resolver.stdlib_bind(&o))?;
        return rb.type_param_variance.get(name_str.as_str()).cloned();
    }
    b.bind.type_param_variance.get(name_str.as_str()).cloned()
}

fn args_fit(
    bind: Option<&BindView>,
    name: varn_core::Atom,
    origin: Option<varn_core::Atom>,
    declared: &[CheckerTyId],
    given: &[CheckerTyId],
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    let vars = measured_variance(bind, name, origin).unwrap_or_default();
    declared.len() == given.len()
        && declared
            .iter()
            .zip(given)
            .enumerate()
            .all(|(i, (x, y))| match vars.get(i) {
                Some(Variance::Covariant) => super::core::types_compatible_impl(
                    &t(*x),
                    &t(*y),
                    bind,
                    cache,
                    in_progress,
                    table,
                ),
                Some(Variance::Contravariant) => super::core::types_compatible_impl(
                    &t(*y),
                    &t(*x),
                    bind,
                    cache,
                    in_progress,
                    table,
                ),
                _ => super::arrays::same_element(&t(*x), &t(*y), bind, cache, in_progress, table),
            })
}

fn upcast_to(
    bind: Option<&BindView>,
    target: (varn_core::Atom, Option<varn_core::Atom>),
    source: (varn_core::Atom, Option<varn_core::Atom>, &[CheckerTyId]),
    table: &CheckerTyTable,
) -> Option<Vec<CheckerTyId>> {
    let b = bind?;
    let target_name = resolve_atom(bind, target.0)?;
    let target_origin = target.1.and_then(|o| resolve_atom(bind, o));
    let source_name = resolve_atom(bind, source.0)?;
    let source_origin = source.1.and_then(|o| resolve_atom(bind, o));
    super::heritage::upcast_args(
        b,
        &source_name,
        source_origin.as_deref(),
        source.2,
        &target_name,
        target_origin.as_deref(),
        table,
    )
}

pub(super) fn generic_arms(
    d: InternedTypeKind,
    i: InternedTypeKind,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match (d, i) {
        (TypeKind::Generic(n1, a1, o1), TypeKind::Generic(n2, a2, o2)) => {
            let declared = table.get_list(a1).to_vec();
            let given = table.get_list(a2).to_vec();
            if n1 == n2 {
                return Some(args_fit(
                    bind,
                    n1,
                    o1,
                    &declared,
                    &given,
                    cache,
                    in_progress,
                    table,
                ));
            }
            let up = upcast_to(bind, (n1, o1), (n2, o2, &given), table);
            Some(up.is_some_and(|up| {
                args_fit(bind, n1, o1, &declared, &up, cache, in_progress, table)
            }))
        }
        (TypeKind::Generic(n1, a1, o1), TypeKind::Named(n2, o2)) if n1 != n2 => {
            let declared = table.get_list(a1).to_vec();
            let up = upcast_to(bind, (n1, o1), (n2, o2, &[]), table)?;
            Some(args_fit(
                bind,
                n1,
                o1,
                &declared,
                &up,
                cache,
                in_progress,
                table,
            ))
        }
        _ => None,
    }
}
