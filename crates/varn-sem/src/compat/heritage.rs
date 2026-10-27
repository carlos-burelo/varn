use super::resolve::resolve_atom;
use crate::bind::BindView;
use crate::types::{CheckerTyId, CheckerTyTable};
use std::sync::Arc;
use varn_core::TypeKind;

pub(super) fn origin_matches(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b || a.ends_with(b) || b.ends_with(a),
        _ => true,
    }
}

fn is_closed(
    id: CheckerTyId,
    params: &[Arc<str>],
    bind: &BindView,
    table: &CheckerTyTable,
) -> bool {
    match table.get(id) {
        TypeKind::Primitive(_) | TypeKind::Literal(_) => true,
        TypeKind::Named(name, _) => {
            resolve_atom(Some(bind), name).is_some_and(|n| !params.iter().any(|p| p.as_ref() == n))
        }
        TypeKind::Array(elem) => is_closed(elem, params, bind, table),
        TypeKind::Generic(_, list, _) | TypeKind::Tuple(list) | TypeKind::Union(list) => table
            .get_list(list)
            .iter()
            .all(|e| is_closed(*e, params, bind, table)),
        TypeKind::This
        | TypeKind::Intersection(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Record(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => false,
    }
}

fn substitute(
    arg: CheckerTyId,
    params: &[Arc<str>],
    given: &[CheckerTyId],
    bind: &BindView,
    table: &CheckerTyTable,
) -> Option<CheckerTyId> {
    if let TypeKind::Named(name, _) = table.get(arg)
        && let Some(name) = resolve_atom(Some(bind), name)
        && let Some(index) = params.iter().position(|p| p.as_ref() == name)
    {
        return given.get(index).copied();
    }
    is_closed(arg, params, bind, table).then_some(arg)
}

pub(super) fn upcast_args(
    bind: &BindView,
    name: &str,
    origin: Option<&str>,
    args: &[CheckerTyId],
    target: &str,
    target_origin: Option<&str>,
    table: &CheckerTyTable,
) -> Option<Vec<CheckerTyId>> {
    let mut seen: Vec<(String, Option<String>)> = Vec::new();
    let mut pending = vec![(name.to_owned(), origin.map(str::to_owned), args.to_vec())];
    while let Some((name, origin, args)) = pending.pop() {
        if name == target && origin_matches(origin.as_deref(), target_origin) {
            return Some(args);
        }
        if seen.iter().any(|(n, o)| *n == name && *o == origin) {
            continue;
        }
        seen.push((name.clone(), origin.clone()));
        let Some(params) = bind.type_param_names(&name, origin.as_deref()) else {
            continue;
        };
        for parent in bind.heritage(&name, origin.as_deref()) {
            let mapped: Option<Vec<CheckerTyId>> = parent
                .args
                .iter()
                .map(|a| substitute(a.0, &params, &args, bind, table))
                .collect();
            if let Some(mapped) = mapped {
                let parent_origin = parent
                    .origin
                    .as_deref()
                    .map(str::to_owned)
                    .or_else(|| origin.clone());
                pending.push((parent.name.to_string(), parent_origin, mapped));
            }
        }
    }
    None
}
