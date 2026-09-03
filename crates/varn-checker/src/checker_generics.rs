use crate::binder::{pattern_lead_name, BindResult};
use crate::checker::Checker;
use crate::symbol::SymbolKind;
use crate::types::{FunctionParam, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Arg, ArrowBody, Expr, ExprKind, Param, Stmt, StmtKind, TypeNode};
use varn_core::TypeKind;

pub(crate) fn build_generic_mapping(
    class_name: &str,
    type_args: &[Type],
    checker: &mut Checker,
    bind: &BindResult,
) -> FxHashMap<Rc<str>, Type> {
    let type_params = checker.symbol_type_params_any(class_name, bind);
    type_params
        .iter()
        .zip(type_args.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub(crate) fn build_call_mapping(
    callee: &Expr,
    type_args: &[TypeNode],
    args: &[Arg],
    ft: &FunctionType,
    checker: &mut Checker,
    bind: &BindResult,
) -> FxHashMap<Rc<str>, Type> {
    let fn_type_params: Vec<Rc<str>> = if !ft.type_params.is_empty() {
        ft.type_params.clone()
    } else if let ExprKind::Identifier { name } = &callee.kind {
        checker.symbol_type_params(name.as_ref(), SymbolKind::Function, bind)
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
    type_params: &[Rc<str>],
    param_types: &[FunctionParam],
    args: &[Arg],
    checker: &mut Checker,
    bind: &BindResult,
) -> FxHashMap<Rc<str>, Type> {
    let mut mapping = FxHashMap::default();

    for (param, arg) in param_types.iter().zip(args.iter()) {
        let is_arrow = match arg {
            Arg::Positional(e) => matches!(e.kind, ExprKind::Arrow { .. }),
            Arg::Named { value, .. } => matches!(value.kind, ExprKind::Arrow { .. }),
            _ => false,
        };
        if is_arrow {
            continue;
        }
        let arg_ty = match arg {
            Arg::Positional(e) => checker.infer_type(e, bind),
            Arg::Named { value, .. } => checker.infer_type(value, bind),
            Arg::Spread(_) => continue,
        };
        collect_type_inferences(&param.ty, &arg_ty, type_params, &mut mapping);
    }

    for (param, arg) in param_types.iter().zip(args.iter()) {
        let is_arrow = match arg {
            Arg::Positional(e) => matches!(e.kind, ExprKind::Arrow { .. }),
            Arg::Named { value, .. } => matches!(value.kind, ExprKind::Arrow { .. }),
            _ => false,
        };
        if !is_arrow {
            continue;
        }

        let mapped_param_ty = map_generics_cached(checker, &param.ty, &mapping);
        let arg_ty = match arg {
            Arg::Positional(e) => {
                if let TypeKind::Fn(expected_fn) = &mapped_param_ty.0 {
                    if let Some(concrete) = infer_arrow_with_context(e, expected_fn, checker, bind)
                    {
                        collect_type_inferences(&param.ty, &concrete, type_params, &mut mapping);
                        continue;
                    }
                }
                checker.infer_type(e, bind)
            }
            Arg::Named { value, .. } => checker.infer_type(value, bind),
            Arg::Spread(_) => continue,
        };
        collect_type_inferences(&param.ty, &arg_ty, type_params, &mut mapping);
    }

    mapping
}

fn infer_arrow_with_context(
    expr: &Expr,
    expected_fn: &FunctionType,
    checker: &mut Checker,
    bind: &BindResult,
) -> Option<Type> {
    let ExprKind::Arrow { params, body, .. } = &expr.kind else {
        return None;
    };

    let mut actual_params = Vec::new();
    for (ap, ep) in params.iter().zip(expected_fn.params.iter()) {
        let explicit_ty = ap
            .type_ann
            .as_ref()
            .or(match &ap.pattern {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            })
            .map(|m| checker.resolve_type_node_cached(m, bind));

        actual_params.push(FunctionParam {
            name: Some(Rc::from(pattern_lead_name(&ap.pattern))),
            ty: explicit_ty.unwrap_or_else(|| ep.ty.clone()),
            optional: ap.is_optional,
            is_rest: ap.is_rest,
        });
    }

    let arrow_scope = find_arrow_scope(checker.current_scope, params, bind);
    let saved_scope = checker.current_scope;

    if let Some(scope_id) = arrow_scope {
        checker.current_scope = scope_id;
        for (ap, ep) in params.iter().zip(expected_fn.params.iter()) {
            let name = pattern_lead_name(&ap.pattern);
            if !name.is_empty() && name != "_" {
                let scope = bind.scopes.get(scope_id);
                if let Some(sym_id) = scope.resolve(name, &bind.scopes) {
                    let explicit_ty = ap
                        .type_ann
                        .as_ref()
                        .or(match &ap.pattern {
                            varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                type_ann.as_ref()
                            }
                            _ => None,
                        })
                        .map(|m| checker.resolve_type_node_cached(m, bind));

                    let ty = explicit_ty.unwrap_or_else(|| ep.ty.clone());
                    checker.symbol_types.insert(sym_id, ty);
                }
            }
        }
    }

    let ret_ty = match body.as_ref() {
        ArrowBody::Expr(e) => {
            let saved_pipeline = checker.in_pipeline_rhs;
            let saved_pipe_ty = checker.pipeline_value_type.clone();
            checker.in_pipeline_rhs = false;
            checker.pipeline_value_type = None;
            let t = checker.infer_type(e, bind);
            checker.in_pipeline_rhs = saved_pipeline;
            checker.pipeline_value_type = saved_pipe_ty;
            t
        }
        ArrowBody::Block(s) => {
            let mut returns = Vec::new();
            collect_returns(s, &mut returns, checker, bind);
            if returns.is_empty() {
                Type::Void
            } else if returns.len() == 1 {
                returns.pop().expect("returns len==1 but pop failed")
            } else {
                Type::union(returns)
            }
        }
    };

    if arrow_scope.is_some() {
        checker.current_scope = saved_scope;
    }

    Some(Type::fn_(FunctionType {
        params: actual_params,
        return_type: Box::new(ret_ty),
        is_arrow: true,
        type_params: Vec::new(),
    }))
}

pub(crate) fn find_arrow_scope(
    current_scope: crate::scope::ScopeId,
    params: &[Param],
    bind: &BindResult,
) -> Option<crate::scope::ScopeId> {
    if params.is_empty() {
        return None;
    }
    let param_names: Vec<&str> = params
        .iter()
        .map(|p| pattern_lead_name(&p.pattern))
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
            if !child_scope.bindings.contains_key(*name) {
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

fn collect_returns(stmt: &Stmt, out: &mut Vec<Type>, checker: &mut Checker, bind: &BindResult) {
    match &stmt.kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_returns(s, out, checker, bind);
            }
        }
        StmtKind::Return { argument } => {
            if let Some(val_expr) = argument {
                out.push(checker.infer_type(val_expr, bind));
            } else {
                out.push(Type::Void);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            collect_returns(consequent, out, checker, bind);
            if let Some(alt) = alternate {
                collect_returns(alt, out, checker, bind);
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_type_inferences(
    expected: &Type,
    actual: &Type,
    params: &[Rc<str>],
    out: &mut FxHashMap<Rc<str>, Type>,
) {
    match &expected.0 {
        TypeKind::Named(name, _origin) if params.contains(name) => {
            let entry = out.entry(name.clone()).or_insert_with(|| actual.clone());
            if entry != actual {
                *entry = Type::union(vec![entry.clone(), actual.clone()]);
            }
        }
        TypeKind::Generic(_, e_args, _) => {
            if let TypeKind::Generic(_, a_args, _) = &actual.0 {
                for (ea, aa) in e_args.iter().zip(a_args.iter()) {
                    collect_type_inferences(ea, aa, params, out);
                }
            }
        }
        TypeKind::Array(e_inner) => {
            if let TypeKind::Array(a_inner) = &actual.0 {
                collect_type_inferences(e_inner, a_inner, params, out);
            }
        }
        TypeKind::Fn(e_ft) => {
            if let TypeKind::Fn(a_ft) = &actual.0 {
                for (ep, ap) in e_ft.params.iter().zip(a_ft.params.iter()) {
                    collect_type_inferences(&ep.ty, &ap.ty, params, out);
                }
                collect_type_inferences(&e_ft.return_type, &a_ft.return_type, params, out);
            }
        }
        TypeKind::Union(e_members) => {
            if let TypeKind::Union(a_members) = &actual.0 {
                for (ea, aa) in e_members.iter().zip(a_members.iter()) {
                    collect_type_inferences(ea, aa, params, out);
                }
            }
        }
        _ => {}
    }
}

fn is_generic_possible(ty: &Type) -> bool {
    !matches!(&ty.0, TypeKind::Intrinsic(_) | TypeKind::This)
}

pub(crate) fn map_generics_cached(
    checker: &mut Checker,
    base: &Type,
    mapping: &FxHashMap<Rc<str>, Type>,
) -> Type {
    if mapping.is_empty() || !is_generic_possible(base) {
        return base.clone();
    }

    if let TypeKind::Named(n, _) = &base.0 {
        if let Some(t) = mapping.get(n) {
            return t.clone();
        } else {
            return base.clone();
        }
    }

    let sorted_args: Vec<Type> = {
        let mut pairs: Vec<(&Rc<str>, &Type)> = mapping.iter().collect();
        pairs.sort_by_key(|(a, _)| *a);
        pairs.into_iter().map(|(_, v)| v.clone()).collect()
    };
    let key = (base.clone(), sorted_args);
    if let Some(cached) = checker.map_generics_cache.get(&key) {
        return cached.clone();
    }
    let result = base.map_generics(mapping);
    checker.map_generics_cache.insert(key, result.clone());
    result
}
