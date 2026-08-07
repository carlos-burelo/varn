use crate::types::{Type, TypeContext};
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
) -> Type {
    if let TypeKind::Union(members) = &check.0 {
        if matches!(&check_node.kind, TypeKind::Named(_, None)) {
            if let TypeKind::Named(var_name, None) = &check_node.kind {
                let results: Vec<Type> = members
                    .iter()
                    .map(|m| {
                        let dist_ctx = AliasSubstitutionContext {
                            inner: ctx,
                            params: vec![var_name.clone()],
                            args: vec![m.clone()],
                        };
                        let mut infer_bindings = FxHashMap::default();
                        let extends_ty = resolve_extends_with_infer(
                            extends,
                            Some(&dist_ctx),
                            &mut infer_bindings,
                            m,
                        );
                        if type_satisfies_extends(m, &extends_ty) {
                            resolve_with_infer_ctx(true_type, Some(&dist_ctx), infer_bindings)
                        } else {
                            resolve_type_node(false_type, Some(&dist_ctx))
                        }
                    })
                    .collect();
                return collapse_union_never(results);
            }
        }
    }

    let mut infer_bindings = FxHashMap::default();
    let extends_ty = resolve_extends_with_infer(extends, ctx, &mut infer_bindings, check);
    if type_satisfies_extends(check, &extends_ty) {
        resolve_with_infer_ctx(true_type, ctx, infer_bindings)
    } else {
        resolve_type_node(false_type, ctx)
    }
}

fn resolve_with_infer_ctx(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    bindings: FxHashMap<String, Type>,
) -> Type {
    if bindings.is_empty() {
        resolve_type_node(node, ctx)
    } else {
        let infer_ctx = InferBindingContext {
            inner: ctx,
            bindings,
        };
        resolve_type_node(node, Some(&infer_ctx))
    }
}

fn collapse_union_never(results: Vec<Type>) -> Type {
    use varn_core::TypeTag;
    let non_never: Vec<Type> = results
        .into_iter()
        .filter(|t| !matches!(&t.0, TypeKind::Intrinsic(TypeTag::Never)))
        .collect();
    match non_never.len() {
        0 => Type::Never,
        1 => non_never.into_iter().next().unwrap(),
        _ => Type::union(non_never),
    }
}

fn resolve_extends_with_infer(
    node: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    bindings: &mut FxHashMap<String, Type>,
    check: &Type,
) -> Type {
    match &node.kind {
        TypeKind::Infer(name) => {
            bindings.insert(name.clone(), check.clone());
            check.clone()
        }
        TypeKind::Generic(name, args, _) => {
            if let TypeKind::Generic(check_name, check_args, _) = &check.0 {
                if check_name.as_ref() == name && check_args.len() == args.len() {
                    for (arg_node, check_arg) in args.iter().zip(check_args.iter()) {
                        resolve_extends_with_infer(arg_node, ctx, bindings, check_arg);
                    }
                }
            }
            resolve_type_node(node, ctx)
        }
        TypeKind::Array(inner) => {
            if let TypeKind::Array(check_inner) = &check.0 {
                resolve_extends_with_infer(inner, ctx, bindings, check_inner);
            }
            resolve_type_node(node, ctx)
        }
        TypeKind::Fn((params, ret)) => {
            if let TypeKind::Fn(ft) = &check.0 {
                resolve_extends_with_infer(ret, ctx, bindings, &ft.return_type);

                for (param_node, check_param) in params.iter().zip(ft.params.iter()) {
                    if let Some(constraint) = &param_node.constraint {
                        resolve_extends_with_infer(constraint, ctx, bindings, &check_param.ty);
                    }
                }
            }
            resolve_type_node(node, ctx)
        }
        _ => resolve_type_node(node, ctx),
    }
}

fn type_satisfies_extends(check: &Type, extends: &Type) -> bool {
    use varn_core::TypeTag;
    if matches!(&check.0, TypeKind::Intrinsic(TypeTag::Never)) {
        return true;
    }

    if matches!(&check.0, TypeKind::Intrinsic(TypeTag::Dynamic))
        || matches!(&extends.0, TypeKind::Intrinsic(TypeTag::Dynamic))
    {
        return true;
    }
    match (&check.0, &extends.0) {
        (_, TypeKind::Union(members)) => members.iter().any(|m| type_satisfies_extends(check, m)),

        (TypeKind::Generic(cn, _, _), TypeKind::Generic(en, _, _)) => cn == en,

        (TypeKind::Named(cn, _), TypeKind::Named(en, _)) => cn == en,

        _ => std::mem::discriminant(&check.0) == std::mem::discriminant(&extends.0),
    }
}
