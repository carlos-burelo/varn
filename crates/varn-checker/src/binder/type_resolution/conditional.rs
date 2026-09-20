use crate::types::{CheckerTyId, CheckerTyTable, Type, TypeContext};
use rustc_hash::FxHashMap;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

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
    if let TypeKind::Union(list) = table.get(check.0).clone() {
        if matches!(&check_node.kind, TypeKind::Named(_, None)) {
            if let TypeKind::Named(var_name, None) = &check_node.kind {
                let var_name_str = ctx
                    .and_then(|c| c.interner())
                    .and_then(|i| i.try_resolve(*var_name))
                    .unwrap_or("")
                    .to_owned();
                let members = table.get_list(list).to_vec();
                let results: Vec<Type> = members
                    .into_iter()
                    .map(|m_id| {
                        let m = Type(m_id, false);
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
                            resolve_with_infer_ctx(
                                true_type,
                                Some(&dist_ctx),
                                infer_bindings,
                                table,
                            )
                        } else {
                            resolve_type_node(false_type, Some(&dist_ctx), table)
                        }
                    })
                    .collect();
                return collapse_union_never(results, table);
            }
        }
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
            if let TypeKind::Generic(check_name, check_args, _) = table.get(check.0).clone() {
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
                                &Type(*check_arg, false),
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
                let check_inner = Type(check_inner, false);
                resolve_extends_with_infer(inner, ctx, bindings, &check_inner, table);
            }
            resolve_type_node(node, ctx, table)
        }
        TypeKind::Fn((params, ret)) => {
            if let TypeKind::Fn(fid) = table.get(check.0) {
                let ft = table.get_function(fid).clone();
                let ret_ty = Type(ft.return_type, false);
                resolve_extends_with_infer(ret, ctx, bindings, &ret_ty, table);

                for (param_node, check_param) in params.iter().zip(ft.params.iter()) {
                    if let Some(constraint) = &param_node.constraint {
                        let param_ty = Type(check_param.ty, false);
                        resolve_extends_with_infer(constraint, ctx, bindings, &param_ty, table);
                    }
                }
            }
            resolve_type_node(node, ctx, table)
        }
        _ => resolve_type_node(node, ctx, table),
    }
}

fn type_satisfies_extends(check: &Type, extends: &Type, table: &CheckerTyTable) -> bool {
    if check.0 == CheckerTyId::NEVER {
        return true;
    }

    if check.0 == CheckerTyId::DYNAMIC || extends.0 == CheckerTyId::DYNAMIC {
        return true;
    }

    if let (TypeKind::Intrinsic(t1), TypeKind::Intrinsic(t2)) =
        (table.get(check.0), table.get(extends.0))
    {
        return t1 == t2;
    }

    match (table.get(check.0).clone(), table.get(extends.0).clone()) {
        (_, TypeKind::Union(list)) => table
            .get_list(list)
            .iter()
            .any(|m| type_satisfies_extends(check, &Type(*m, false), table)),

        (TypeKind::Generic(cn, ca, _), TypeKind::Generic(en, ea, _)) => {
            let ca_ids = table.get_list(ca).to_vec();
            let ea_ids = table.get_list(ea).to_vec();
            cn == en
                && ca_ids.len() == ea_ids.len()
                && ca_ids
                    .iter()
                    .zip(ea_ids.iter())
                    .all(|(c, e)| type_satisfies_extends(&Type(*c, false), &Type(*e, false), table))
        }

        (TypeKind::Named(cn, _), TypeKind::Named(en, _)) => cn == en,

        (TypeKind::Array(c), TypeKind::Array(e)) => {
            type_satisfies_extends(&Type(c, false), &Type(e, false), table)
        }

        (TypeKind::Fn(f_check), TypeKind::Fn(f_extends)) => {
            let check_ret = Type(table.get_function(f_check).return_type, false);
            let extends_ret = Type(table.get_function(f_extends).return_type, false);
            type_satisfies_extends(&check_ret, &extends_ret, table)
        }

        _ => check == extends,
    }
}
