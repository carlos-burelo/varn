use super::scalar::t;
use super::variance::Variance;
use crate::bind::BindView;
use crate::types::InternedTypeKind;
use crate::types::{CheckerTyTable, Type};
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

pub(super) fn generic_arms(
    d: InternedTypeKind,
    i: InternedTypeKind,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match (d, i) {
        (TypeKind::Generic(n1, a1, o1), TypeKind::Generic(n2, a2, _o2)) => {
            let l1 = table.get_list(a1);
            let l2 = table.get_list(a2);
            if n1 == n2 {
                let vars = measured_variance(bind, n1, o1).unwrap_or_default();
                Some(
                    l1.len() == l2.len()
                        && l1.to_vec().iter().zip(l2.to_vec().iter()).enumerate().all(
                            |(i, (x, y))| match vars.get(i) {
                                Some(Variance::Covariant) => super::core::types_compatible_impl(
                                    &t(*x),
                                    &t(*y),
                                    bind,
                                    cache,
                                    in_progress,
                                    table,
                                ),
                                Some(Variance::Contravariant) => {
                                    super::core::types_compatible_impl(
                                        &t(*y),
                                        &t(*x),
                                        bind,
                                        cache,
                                        in_progress,
                                        table,
                                    )
                                }
                                _ => super::arrays::same_element(
                                    &t(*x),
                                    &t(*y),
                                    bind,
                                    cache,
                                    in_progress,
                                    table,
                                ),
                            },
                        ),
                )
            } else {
                Some(false)
            }
        }
        _ => None,
    }
}
