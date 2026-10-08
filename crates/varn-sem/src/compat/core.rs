use super::arrays::array_arms;
use super::generics::generic_arms;
use super::nominal::nominal_arms;
use super::objects::object_arm;
use super::resolve::resolve_atom;
use super::scalar::{is_simple_type, simple_types_compatible, t};
use super::unions::union_arms;
use crate::bind::{BindResult, BindView};
use crate::types::{CheckerTyTable, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub fn types_compatible(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    table: &CheckerTyTable,
) -> bool {
    if declared.0 == inferred.0 {
        return true;
    }
    let mut cache = FxHashMap::default();
    types_compatible_with_cache(declared, inferred, bind, &mut cache, table)
}

pub fn types_compatible_with_cache(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    table: &CheckerTyTable,
) -> bool {
    if declared.0 == inferred.0 {
        return true;
    }
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared, table) && is_simple_type(inferred, table) {
        return simple_types_compatible(declared, inferred, table);
    }
    let mut in_progress = FxHashSet::default();
    types_compatible_impl(declared, inferred, bind, cache, &mut in_progress, table)
}

pub(super) fn types_compatible_impl(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    if declared.0 == inferred.0 {
        return true;
    }
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared, table) && is_simple_type(inferred, table) {
        return simple_types_compatible(declared, inferred, table);
    }
    let key = (
        *declared,
        *inferred,
        bind.map_or(0usize, |b| b.bind as *const BindResult as usize),
    );
    if let Some(cached) = cache.get(&key) {
        return *cached;
    }
    if !in_progress.insert(key) {
        return true;
    }

    let d = table.get(declared.0);
    let i = table.get(inferred.0);
    let result = match (d, i) {
        (TypeKind::Primitive(varn_core::LangPrimitive::Dynamic), _)
        | (_, TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)) => true,

        (_, TypeKind::Primitive(varn_core::LangPrimitive::Never)) => true,

        (a, b) if a == b => true,

        (
            TypeKind::Primitive(_) | TypeKind::Builtin(_),
            TypeKind::Primitive(_) | TypeKind::Builtin(_),
        ) => simple_types_compatible(declared, inferred, table),
        (TypeKind::Primitive(p), TypeKind::Literal(l)) => {
            use varn_core::LangPrimitive as P;
            let base = l.base();
            p == base || (matches!(p, P::Decimal | P::BigInt) && base == P::Int)
        }
        (TypeKind::Primitive(varn_core::LangPrimitive::Str), TypeKind::TemplateLiteral(_)) => true,
        (TypeKind::TemplateLiteral(a), TypeKind::TemplateLiteral(b)) => a == b,

        (lang @ (TypeKind::Primitive(_) | TypeKind::Builtin(_)), TypeKind::Named(name, _))
        | (TypeKind::Named(name, _), lang @ (TypeKind::Primitive(_) | TypeKind::Builtin(_)))
            if resolve_atom(bind, name)
                .as_deref()
                .and_then(TypeKind::of_lang_name)
                .is_some_and(|k| k == lang) =>
        {
            true
        }

        (TypeKind::Tuple(decl_elems), TypeKind::Tuple(inf_elems)) => {
            let decl_ids = table.get_list(decl_elems).to_vec();
            let inf_ids = table.get_list(inf_elems).to_vec();
            decl_ids.len() == inf_ids.len()
                && decl_ids.iter().zip(inf_ids.iter()).all(|(dd, ii)| {
                    types_compatible_impl(&t(*dd), &t(*ii), bind, cache, in_progress, table)
                })
        }

        (TypeKind::Intersection(decl_members), _) => table
            .get_list(decl_members)
            .to_vec()
            .iter()
            .all(|m| types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)),

        (_, TypeKind::Intersection(inf_members)) => table
            .get_list(inf_members)
            .to_vec()
            .iter()
            .any(|m| types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)),

        _ => {
            if let Some(r) = array_arms(d, i, bind, cache, in_progress, table) {
                r
            } else if let Some(r) = generic_arms(d, i, bind, cache, in_progress, table) {
                r
            } else if let Some(r) =
                union_arms(declared, inferred, d, i, bind, cache, in_progress, table)
            {
                r
            } else if let Some(r) =
                nominal_arms(declared, inferred, d, i, bind, cache, in_progress, table)
            {
                r
            } else if let (TypeKind::Object(decl_fields), TypeKind::Object(inf_fields)) = (d, i) {
                object_arm(decl_fields, inf_fields, bind, cache, in_progress, table)
            } else {
                false
            }
        }
    };

    in_progress.remove(&key);
    cache.insert(key, result);
    result
}
