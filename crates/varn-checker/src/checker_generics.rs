use crate::binder::{pattern_lead_name, BindResult};
use crate::checker::Checker;
use crate::symbol::SymbolKind;
use crate::types::{CheckerTyTable, FunctionParam, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{Arg, ArrowBody, ExprId, ExprKind, Param, StmtId, StmtKind, TypeNode};
use varn_core::TypeKind;

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
            _ => false,
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
            &Type(param.ty, false),
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
            _ => false,
        };
        if !is_arrow {
            continue;
        }

        let mapped_param_ty = map_generics_cached(checker, &Type(param.ty, false), &mapping);
        let arg_ty = match arg {
            Arg::Positional(e) => {
                let mapped_kind = checker.ty_table.get(mapped_param_ty.0);
                if let TypeKind::Fn(fid) = mapped_kind {
                    let expected_fn = checker.ty_table.get_function(fid).clone();
                    if let Some(concrete) =
                        infer_arrow_with_context(*e, &expected_fn, checker, bind)
                    {
                        collect_type_inferences(
                            &Type(param.ty, false),
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
            &Type(param.ty, false),
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
            .or(match &ap.pattern {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            })
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
                        .or(match &ap.pattern {
                            varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                type_ann.as_ref()
                            }
                            _ => None,
                        })
                        .map(|m| checker.resolve_type_node_cached(m, bind));

                    let ty = explicit_ty.unwrap_or(Type(ep.ty, false));
                    checker.symbol_types.insert(sym_id, ty);
                }
            }
        }
    }

    let ret_ty = match &body {
        ArrowBody::Expr(e) => {
            let e = *e;
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
            let s = *s;
            let mut returns = Vec::new();
            collect_returns(s, &mut returns, checker, bind);
            if returns.is_empty() {
                Type::Void
            } else if returns.len() == 1 {
                returns.pop().expect("returns len==1 but pop failed")
            } else {
                Type::union(
                    returns,
                    &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                )
            }
        }
    };

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
    current_scope: crate::scope::ScopeId,
    params: &[Param],
    bind: &BindResult,
) -> Option<crate::scope::ScopeId> {
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

fn collect_returns(stmt: StmtId, out: &mut Vec<Type>, checker: &mut Checker, bind: &BindResult) {
    match &checker.ast_arena.stmt(stmt).kind {
        StmtKind::Block { stmts } => {
            let stmts = stmts.clone();
            for s in stmts {
                collect_returns(s, out, checker, bind);
            }
        }
        StmtKind::Return { argument } => {
            if let Some(val_expr) = argument {
                let val_expr = *val_expr;
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
            let (consequent, alternate) = (*consequent, *alternate);
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
    params: &[Arc<str>],
    out: &mut FxHashMap<Arc<str>, Type>,
    table: &mut CheckerTyTable,
    interner: &varn_core::AtomInterner,
) {
    let expected_kind = table.get(expected.0);
    match expected_kind {
        TypeKind::Named(name, _origin)
            if params.iter().any(|p| p.as_ref() == interner.resolve(name)) =>
        {
            let name_rc: Arc<str> = Arc::from(interner.resolve(name));
            let entry = out.entry(name_rc).or_insert(*actual);
            if entry != actual {
                *entry = Type::union(vec![*entry, *actual], table);
            }
        }
        TypeKind::Generic(_, e_args, _) => {
            if let TypeKind::Generic(_, a_args, _) = table.get(actual.0) {
                let e_ids = table.get_list(e_args).to_vec();
                let a_ids = table.get_list(a_args).to_vec();
                for (ea, aa) in e_ids.iter().zip(a_ids.iter()) {
                    collect_type_inferences(
                        &Type(*ea, false),
                        &Type(*aa, false),
                        params,
                        out,
                        table,
                        interner,
                    );
                }
            }
        }
        TypeKind::Array(e_inner) => {
            if let TypeKind::Array(a_inner) = table.get(actual.0) {
                collect_type_inferences(
                    &Type(e_inner, false),
                    &Type(a_inner, false),
                    params,
                    out,
                    table,
                    interner,
                );
            }
        }
        TypeKind::Fn(e_fid) => {
            if let TypeKind::Fn(a_fid) = table.get(actual.0) {
                let e_ft = table.get_function(e_fid).clone();
                let a_ft = table.get_function(a_fid).clone();
                for (ep, ap) in e_ft.params.iter().zip(a_ft.params.iter()) {
                    collect_type_inferences(
                        &Type(ep.ty, false),
                        &Type(ap.ty, false),
                        params,
                        out,
                        table,
                        interner,
                    );
                }
                collect_type_inferences(
                    &Type(e_ft.return_type, false),
                    &Type(a_ft.return_type, false),
                    params,
                    out,
                    table,
                    interner,
                );
            }
        }
        TypeKind::Union(e_members) => {
            if let TypeKind::Union(a_members) = table.get(actual.0) {
                let e_ids = table.get_list(e_members).to_vec();
                let a_ids = table.get_list(a_members).to_vec();
                for (ea, aa) in e_ids.iter().zip(a_ids.iter()) {
                    collect_type_inferences(
                        &Type(*ea, false),
                        &Type(*aa, false),
                        params,
                        out,
                        table,
                        interner,
                    );
                }
            }
        }
        _ => {}
    }
}

fn is_generic_possible(ty: &Type, table: &CheckerTyTable) -> bool {
    !matches!(table.get(ty.0), TypeKind::Intrinsic(_) | TypeKind::This)
}

pub(crate) fn map_generics_cached(
    checker: &mut Checker,
    base: &Type,
    mapping: &FxHashMap<Arc<str>, Type>,
) -> Type {
    if mapping.is_empty() || !is_generic_possible(base, &checker.ty_table) {
        return *base;
    }

    // Atom-keyed view of `mapping`, since `Type::map_generics` compares
    // against `TypeKind::Named`'s own `Atom` slot, not a source-text name.
    let atom_mapping: FxHashMap<varn_core::Atom, Type> = mapping
        .iter()
        .map(|(k, v)| (checker.resolver.intern(k), *v))
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
        pairs.sort_by_key(|(a, _)| a.clone());
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
