use crate::checker::Checker;
use crate::checker_type_inferences::collect_type_inferences;
use crate::generic_substitution::map_generics_cached;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_binder::pattern_lead_name;
use varn_core::ast::{Arg, ExprId, ExprKind, Param, TypeNode};
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolKind;
use varn_sem::types::{FunctionParam, FunctionType, Type};

pub(crate) fn build_call_mapping(
    callee: ExprId,
    type_args: &[TypeNode],
    args: &[Arg],
    ft: &FunctionType,
    checker: &mut Checker,
    bind: &BindResult,
) -> FxHashMap<Arc<str>, Type> {
    let fn_type_params: Vec<Arc<str>> = if !ft.type_params.is_empty() {
        ft.type_params.clone()
    } else if let ExprKind::Identifier { name } = &checker.ast_arena.expr(callee).kind {
        checker.symbol_type_params(bind.interner.resolve(*name), SymbolKind::Function, bind)
    } else {
        Vec::new()
    };

    if fn_type_params.is_empty() {
        return FxHashMap::default();
    }

    if !type_args.is_empty() && type_args.len() == fn_type_params.len() {
        fn_type_params
            .iter()
            .zip(
                type_args
                    .iter()
                    .map(|a| checker.resolve_type_node_cached(a, bind)),
            )
            .map(|(k, v)| (k.clone(), v))
            .collect()
    } else if type_args.is_empty() {
        let mut mapping = infer_mapping_from_args(&fn_type_params, &ft.params, args, checker, bind);
        for tp in &fn_type_params {
            mapping.entry(tp.clone()).or_insert(Type::Dynamic);
        }
        mapping
    } else {
        FxHashMap::default()
    }
}

pub(crate) fn infer_mapping_from_args(
    type_params: &[Arc<str>],
    param_types: &[FunctionParam],
    args: &[Arg],
    checker: &mut Checker,
    bind: &BindResult,
) -> FxHashMap<Arc<str>, Type> {
    let mut mapping = FxHashMap::default();

    for (param, arg) in param_types.iter().zip(args.iter()) {
        let is_arrow = match arg {
            Arg::Positional(e) => matches!(checker.ast_arena.expr(*e).kind, ExprKind::Arrow { .. }),
            Arg::Named { value, .. } => {
                matches!(checker.ast_arena.expr(*value).kind, ExprKind::Arrow { .. })
            }
            Arg::Spread(_) => false,
        };
        if is_arrow {
            continue;
        }
        let arg_ty = match arg {
            Arg::Positional(e) => checker.infer_type(*e, bind),
            Arg::Named { value, .. } => checker.infer_type(*value, bind),
            Arg::Spread(_) => continue,
        };
        collect_type_inferences(
            &Type::resolved(param.ty),
            &arg_ty,
            type_params,
            &mut mapping,
            &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
            &bind.interner,
        );
    }

    for (param, arg) in param_types.iter().zip(args.iter()) {
        let is_arrow = match arg {
            Arg::Positional(e) => matches!(checker.ast_arena.expr(*e).kind, ExprKind::Arrow { .. }),
            Arg::Named { value, .. } => {
                matches!(checker.ast_arena.expr(*value).kind, ExprKind::Arrow { .. })
            }
            Arg::Spread(_) => false,
        };
        if !is_arrow {
            continue;
        }

        let mapped_param_ty = map_generics_cached(checker, &Type::resolved(param.ty), &mapping);
        let arg_ty = match arg {
            Arg::Positional(e) => {
                let mapped_kind = checker.ty_table.get(mapped_param_ty.0);
                if let TypeKind::Fn(fid) = mapped_kind {
                    let expected_fn = checker.ty_table.get_function(fid).clone();
                    if let Some(concrete) =
                        infer_arrow_with_context(*e, &expected_fn, checker, bind)
                    {
                        collect_type_inferences(
                            &Type::resolved(param.ty),
                            &concrete,
                            type_params,
                            &mut mapping,
                            &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                            &bind.interner,
                        );
                        continue;
                    }
                }
                checker.infer_type(*e, bind)
            }
            Arg::Named { value, .. } => checker.infer_type(*value, bind),
            Arg::Spread(_) => continue,
        };
        collect_type_inferences(
            &Type::resolved(param.ty),
            &arg_ty,
            type_params,
            &mut mapping,
            &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
            &bind.interner,
        );
    }

    mapping
}

fn infer_arrow_with_context(
    expr: ExprId,
    expected_fn: &FunctionType,
    checker: &mut Checker,
    bind: &BindResult,
) -> Option<Type> {
    let ExprKind::Arrow { params, body, .. } = &checker.ast_arena.expr(expr).kind else {
        return None;
    };
    let params = params.clone();
    let body = (**body).clone();

    let mut actual_params = Vec::new();
    for (ap, ep) in params.iter().zip(expected_fn.params.iter()) {
        let explicit_ty = ap
            .type_ann
            .as_ref()
            .map(|m| checker.resolve_type_node_cached(m, bind));

        actual_params.push(FunctionParam {
            name: Some(Arc::from(pattern_lead_name(&ap.pattern, &bind.interner))),
            ty: explicit_ty.map(|t| t.0).unwrap_or(ep.ty),
            optional: ap.is_optional,
            is_rest: ap.is_rest,
        });
    }

    let arrow_scope = find_arrow_scope(checker.current_scope, &params, bind);
    let saved_scope = checker.current_scope;

    if let Some(scope_id) = arrow_scope {
        checker.current_scope = scope_id;
        for (ap, ep) in params.iter().zip(expected_fn.params.iter()) {
            let name = pattern_lead_name(&ap.pattern, &bind.interner);
            if !name.is_empty() && name != "_" {
                let scope = bind.scopes.get(scope_id);
                if let Some(sym_id) = bind
                    .interner
                    .get(name)
                    .and_then(|atom| scope.resolve(atom, &bind.scopes))
                {
                    let explicit_ty = ap
                        .type_ann
                        .as_ref()
                        .map(|m| checker.resolve_type_node_cached(m, bind));

                    let ty = explicit_ty.unwrap_or(Type::resolved(ep.ty));
                    checker.symbol_types.insert(sym_id, ty);
                }
            }
        }
    }

    let ret_ty = crate::checker_expressions::infer::arrow_body_return_type(body, checker, bind);

    if arrow_scope.is_some() {
        checker.current_scope = saved_scope;
    }

    Some(Type::fn_(
        FunctionType {
            params: actual_params,
            return_type: ret_ty.0,
            is_arrow: true,
            type_params: Vec::new(),
        },
        &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
    ))
}

pub(crate) fn find_arrow_scope(
    current_scope: varn_sem::scope::ScopeId,
    params: &[Param],
    bind: &BindResult,
) -> Option<varn_sem::scope::ScopeId> {
    if params.is_empty() {
        return None;
    }
    let param_names: Vec<&str> = params
        .iter()
        .map(|p| pattern_lead_name(&p.pattern, &bind.interner))
        .filter(|name| !name.is_empty() && *name != "_")
        .collect();

    if param_names.is_empty() {
        return None;
    }

    let children = &bind.scopes.get(current_scope).children;
    for &child_id in children {
        let child_scope = bind.scopes.get(child_id);
        let mut matches = true;
        for name in &param_names {
            let found = bind
                .interner
                .get(name)
                .is_some_and(|atom| child_scope.bindings.contains_key(&atom));
            if !found {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(child_id);
        }
    }
    None
}
