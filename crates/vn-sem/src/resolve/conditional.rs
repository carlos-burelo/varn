use crate::types::{CheckerTyId, CheckerTyTable, Type, TypeContext};
use rustc_hash::FxHashMap;
use vn_core::ast::TypeNode;
use vn_core::TypeKind;

use super::contexts::{AliasSubstitutionContext, InferBindingContext};
use super::resolve_type_node;

pub(super) fn resolve_conditional(
    check_node: &TypeNode,
    check: &Type,
    extends: &TypeNode,
    true_type: &TypeNode,
    false_type: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    if mentions_type_parameter(check, ctx, table) && pattern_is_deferrable(extends) {
        let extends_ty = resolve_pattern(extends, ctx, table);
        let true_ty = resolve_type_node(true_type, ctx, table);
        let false_ty = resolve_type_node(false_type, ctx, table);
        return Type::resolved(table.intern(TypeKind::Conditional {
            check: check.0,
            extends: extends_ty.0,
            true_type: true_ty.0,
            false_type: false_ty.0,
        }));
    }
    if let TypeKind::Union(list) = table.get(check.0)
        && matches!(&check_node.kind, TypeKind::Named(_, None))
        && let TypeKind::Named(var_name, None) = &check_node.kind
    {
        let var_name_str = ctx
            .and_then(|c| c.interner())
            .and_then(|i| i.try_resolve(*var_name))
            .unwrap_or("")
            .to_owned();
        let members = table.get_list(list).to_vec();
        let results: Vec<Type> = members
            .into_iter()
            .map(|m_id| {
                let m = Type::resolved(m_id);
                let dist_ctx = AliasSubstitutionContext {
                    inner: ctx,
                    params: vec![var_name_str.clone()],
                    args: vec![m],
                };
                let mut infer_bindings = FxHashMap::default();
                let extends_ty = resolve_extends_with_infer(
                    extends,
                    Some(&dist_ctx),
                    &mut infer_bindings,
                    &m,
                    table,
                );
                if type_satisfies_extends(&m, &extends_ty, table) {
                    resolve_with_infer_ctx(true_type, Some(&dist_ctx), infer_bindings, table)
                } else {
                    resolve_type_node(false_type, Some(&dist_ctx), table)
                }
            })
            .collect();
        return collapse_union_never(results, table);
    }

    let mut infer_bindings = FxHashMap::default();
    let extends_ty = resolve_extends_with_infer(extends, ctx, &mut infer_bindings, check, table);
    if type_satisfies_extends(check, &extends_ty, table) {
        resolve_with_infer_ctx(true_type, ctx, infer_bindings, table)
    } else {
        resolve_type_node(false_type, ctx, table)
    }
}

fn resolve_with_infer_ctx(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    bindings: FxHashMap<String, Type>,
    table: &mut CheckerTyTable,
) -> Type {
    if bindings.is_empty() {
        resolve_type_node(node, ctx, table)
    } else {
        let infer_ctx = InferBindingContext {
            inner: ctx,
            bindings,
        };
        resolve_type_node(node, Some(&infer_ctx), table)
    }
}

fn collapse_union_never(results: Vec<Type>, table: &mut CheckerTyTable) -> Type {
    let non_never: Vec<Type> = results
        .into_iter()
        .filter(|t| t.0 != CheckerTyId::NEVER)
        .collect();
    match non_never.len() {
        0 => Type::Never,
        1 => non_never.into_iter().next().unwrap(),
        _ => Type::union(non_never, table),
    }
}

fn resolve_extends_with_infer(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    bindings: &mut FxHashMap<String, Type>,
    check: &Type,
    table: &mut CheckerTyTable,
) -> Type {
    match &node.kind {
        TypeKind::Infer(name) => {
            let name_str = ctx
                .and_then(|c| c.interner())
                .and_then(|i| i.try_resolve(*name))
                .unwrap_or("")
                .to_owned();
            bindings.insert(name_str, *check);
            *check
        }
        TypeKind::Generic(name, args, _) => {
            let name_str = ctx
                .and_then(|c| c.interner())
                .and_then(|i| i.try_resolve(*name));
            if let TypeKind::Generic(check_name, check_args, _) = table.get(check.0) {
                let check_name_str = ctx
                    .and_then(|c| c.interner())
                    .map(|i| i.resolve(check_name));
                if check_name_str == name_str {
                    let check_arg_ids = table.get_list(check_args).to_vec();
                    if check_arg_ids.len() == args.len() {
                        for (arg_node, check_arg) in args.iter().zip(check_arg_ids.iter()) {
                            resolve_extends_with_infer(
                                arg_node,
                                ctx,
                                bindings,
                                &Type::resolved(*check_arg),
                                table,
                            );
                        }
                    }
                }
            }
            resolve_type_node(node, ctx, table)
        }
        TypeKind::Array(inner) => {
            if let TypeKind::Array(check_inner) = table.get(check.0) {
                let check_inner = Type::resolved(check_inner);
                resolve_extends_with_infer(inner, ctx, bindings, &check_inner, table);
            }
            resolve_type_node(node, ctx, table)
        }
        TypeKind::Fn((params, ret)) => {
            if let TypeKind::Fn(fid) = table.get(check.0) {
                let ft = table.get_function(fid).clone();
                let ret_ty = Type::resolved(ft.return_type);
                resolve_extends_with_infer(ret, ctx, bindings, &ret_ty, table);

                for (param_node, check_param) in params.iter().zip(ft.params.iter()) {
                    if let Some(constraint) = &param_node.constraint {
                        let param_ty = Type::resolved(check_param.ty);
                        resolve_extends_with_infer(constraint, ctx, bindings, &param_ty, table);
                    }
                }
            }
            resolve_type_node(node, ctx, table)
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => resolve_type_node(node, ctx, table),
    }
}

pub(crate) fn type_satisfies_extends(check: &Type, extends: &Type, table: &CheckerTyTable) -> bool {
    if check.0 == CheckerTyId::NEVER {
        return true;
    }

    if check.is_unresolved() || extends.is_unresolved() {
        return true;
    }

    if matches!(table.get(extends.0), TypeKind::Infer(_)) {
        return true;
    }

    if let (TypeKind::Primitive(p1), TypeKind::Primitive(p2)) =
        (table.get(check.0), table.get(extends.0))
    {
        return p1 == p2;
    }

    match (table.get(check.0), table.get(extends.0)) {
        (_, TypeKind::Union(list)) => table
            .get_list(list)
            .iter()
            .any(|m| type_satisfies_extends(check, &Type::resolved(*m), table)),

        (TypeKind::Generic(cn, ca, _), TypeKind::Generic(en, ea, _)) => {
            let ca_ids = table.get_list(ca).to_vec();
            let ea_ids = table.get_list(ea).to_vec();
            cn == en
                && ca_ids.len() == ea_ids.len()
                && ca_ids.iter().zip(ea_ids.iter()).all(|(c, e)| {
                    type_satisfies_extends(&Type::resolved(*c), &Type::resolved(*e), table)
                })
        }

        (TypeKind::Named(cn, _), TypeKind::Named(en, _)) => cn == en,

        (TypeKind::Array(c), TypeKind::Array(e)) => {
            type_satisfies_extends(&Type::resolved(c), &Type::resolved(e), table)
        }

        (TypeKind::Fn(f_check), TypeKind::Fn(f_extends)) => {
            let check_ret = Type::resolved(table.get_function(f_check).return_type);
            let extends_ret = Type::resolved(table.get_function(f_extends).return_type);
            type_satisfies_extends(&check_ret, &extends_ret, table)
        }

        _ => check == extends,
    }
}

fn mentions_type_parameter(
    check: &Type,
    ctx: Option<&dyn TypeContext>,
    table: &CheckerTyTable,
) -> bool {
    let Some(ctx) = ctx else { return false };
    let mut pending = vec![*check];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current.0) {
            continue;
        }
        if let TypeKind::Named(name, None) = table.get(current.0)
            && let Some(text) = table.names().try_resolve(name)
            && ctx.is_type_parameter(text)
        {
            return true;
        }
        pending.extend(current.components(table));
    }
    false
}

fn pattern_is_deferrable(node: &TypeNode) -> bool {
    match &node.kind {
        TypeKind::Infer(_) | TypeKind::Primitive(_) | TypeKind::Named(..) => true,
        TypeKind::Array(inner) => pattern_is_deferrable(inner),
        TypeKind::Generic(_, args, _) => args.iter().all(pattern_is_deferrable),
        TypeKind::Tuple(items) => items.iter().all(pattern_is_deferrable),
        TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => false,
    }
}

fn resolve_pattern(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    match &node.kind {
        TypeKind::Infer(name) => Type::resolved(table.intern(TypeKind::Infer(*name))),
        TypeKind::Array(inner) => {
            let inner = resolve_pattern(inner, ctx, table);
            Type::array(inner, table)
        }
        TypeKind::Generic(_, args, _) => {
            let head = resolve_type_node(node, ctx, table);
            let TypeKind::Generic(name, _, origin) = table.get(head.0) else {
                return head;
            };
            let items: Vec<CheckerTyId> = args
                .iter()
                .map(|arg| resolve_pattern(arg, ctx, table).0)
                .collect();
            let list = table.intern_list(&items);
            Type::resolved(table.intern(TypeKind::Generic(name, list, origin)))
        }
        TypeKind::Tuple(items) => {
            let ids: Vec<CheckerTyId> = items
                .iter()
                .map(|item| resolve_pattern(item, ctx, table).0)
                .collect();
            let list = table.intern_list(&ids);
            Type::resolved(table.intern(TypeKind::Tuple(list)))
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Named(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => resolve_type_node(node, ctx, table),
    }
}

pub(crate) fn evaluate_conditional(
    check: Type,
    extends: Type,
    true_type: Type,
    false_type: Type,
    table: &mut CheckerTyTable,
) -> Type {
    if let TypeKind::Union(list) = table.get(check.0) {
        let members = table.get_list(list).to_vec();
        let results: Vec<Type> = members
            .into_iter()
            .map(|member| {
                let member = Type::resolved(member);
                let on_true = if true_type == check {
                    member
                } else {
                    true_type
                };
                let on_false = if false_type == check {
                    member
                } else {
                    false_type
                };
                evaluate_conditional(member, extends, on_true, on_false, table)
            })
            .collect();
        return collapse_union_never(results, table);
    }
    let mut bindings = FxHashMap::default();
    if match_pattern(extends, check, &mut bindings, table) {
        true_type.map_generics(&bindings, table)
    } else {
        false_type
    }
}

fn contains_infer(ty: Type, table: &CheckerTyTable) -> bool {
    let mut pending = vec![ty];
    while let Some(current) = pending.pop() {
        if matches!(table.get(current.0), TypeKind::Infer(_)) {
            return true;
        }
        pending.extend(current.components(table));
    }
    false
}

fn match_pattern(
    pattern: Type,
    actual: Type,
    bindings: &mut FxHashMap<vn_core::Atom, Type>,
    table: &CheckerTyTable,
) -> bool {
    match (table.get(pattern.0), table.get(actual.0)) {
        (TypeKind::Infer(name), _) => {
            bindings.insert(name, actual);
            true
        }
        (TypeKind::Array(p), TypeKind::Array(a)) => {
            match_pattern(Type::resolved(p), Type::resolved(a), bindings, table)
        }
        (TypeKind::Generic(pn, pa, _), TypeKind::Generic(an, aa, _)) => {
            let (pa, aa) = (table.get_list(pa).to_vec(), table.get_list(aa).to_vec());
            pn == an
                && pa.len() == aa.len()
                && pa.iter().zip(aa.iter()).all(|(p, a)| {
                    match_pattern(Type::resolved(*p), Type::resolved(*a), bindings, table)
                })
        }
        (TypeKind::Tuple(p), TypeKind::Tuple(a)) => {
            let (p, a) = (table.get_list(p).to_vec(), table.get_list(a).to_vec());
            p.len() == a.len()
                && p.iter().zip(a.iter()).all(|(p, a)| {
                    match_pattern(Type::resolved(*p), Type::resolved(*a), bindings, table)
                })
        }
        (
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. },
            _,
        ) => !contains_infer(pattern, table) && type_satisfies_extends(&actual, &pattern, table),
    }
}
