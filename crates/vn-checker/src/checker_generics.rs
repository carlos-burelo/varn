use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use crate::checker_type_inferences::collect_type_inferences;
use crate::generic_substitution::map_generics_cached;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use vn_core::ast::{Arg, ExprId, ExprKind, Param, TypeNode};
use vn_core::TypeKind;
use vn_sem::bind::BindResult;
use vn_sem::symbol::SymbolKind;
use vn_sem::types::{FunctionParam, FunctionType, Type};

pub(crate) fn fn_type_param_names(
    callee: ExprId,
    ft: &FunctionType,
    checker: &mut Checker,
    bind: &BindResult,
) -> Vec<Arc<str>> {
    if !ft.type_params.is_empty() {
        ft.type_params.clone()
    } else if let ExprKind::Identifier { name } = &checker.ast_arena.expr(callee).kind {
        checker.symbol_type_params(bind.interner.resolve(*name), SymbolKind::Function, bind)
    } else {
        Vec::new()
    }
}

pub(crate) fn check_call_type_arg_arity(
    callee: ExprId,
    type_args: &[TypeNode],
    ft: &FunctionType,
    checker: &mut Checker,
    range: &vn_core::source::SourceRange,
    bind: &BindResult,
) {
    if type_args.is_empty() {
        return;
    }
    let expected = fn_type_param_names(callee, ft, checker, bind);
    if expected.len() != type_args.len() {
        checker.emit(
            vn_core::Diagnostic::error(
                vn_core::ErrorCode::WrongTypeArgCount,
                format!(
                    "wrong number of type arguments: expected {} but got {}",
                    expected.len(),
                    type_args.len()
                ),
            )
            .with_range(*range),
        );
    }
}

fn signature_mentions(ft: &FunctionType, name: &str, checker: &Checker, bind: &BindResult) -> bool {
    let mut pending: Vec<Type> = ft
        .params
        .iter()
        .map(|p| Type::resolved(p.ty))
        .chain([Type::resolved(ft.return_type)])
        .collect();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current.0) {
            continue;
        }
        if let TypeKind::Named(atom, _) = checker.ty_table.get(current.0)
            && bind.interner.resolve(atom) == name
        {
            return true;
        }
        pending.extend(current.components(&checker.ty_table));
    }
    false
}

pub(crate) fn build_call_mapping(
    call_index: u32,
    callee: ExprId,
    type_args: &[TypeNode],
    args: &[Arg],
    ft: &FunctionType,
    checker: &mut Checker,
    rec: &mut Recorder,
    bind: &BindResult,
) -> (FxHashMap<Arc<str>, Type>, Vec<Arc<str>>) {
    let fn_type_params = fn_type_param_names(callee, ft, checker, bind);

    if fn_type_params.is_empty() {
        return (FxHashMap::default(), Vec::new());
    }

    if !type_args.is_empty() && type_args.len() == fn_type_params.len() {
        let explicit = fn_type_params
            .iter()
            .zip(
                type_args
                    .iter()
                    .map(|a| checker.resolve_type_node_cached(a, bind)),
            )
            .map(|(k, v)| (k.clone(), v))
            .collect();
        (explicit, Vec::new())
    } else if type_args.is_empty() {
        let mut mapping =
            infer_mapping_from_args(&fn_type_params, &ft.params, args, checker, rec, bind);
        if let Some(expected) = checker
            .expected_type
            .filter(|expected| !expected.is_unresolved())
            .filter(|_| checker.expected_owner == Some(call_index))
        {
            let mut from_return = FxHashMap::default();
            collect_type_inferences(
                &Type::resolved(ft.return_type),
                &expected,
                &fn_type_params,
                &mut from_return,
                &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                &bind.interner,
            );
            for (name, ty) in from_return {
                mapping.entry(name).or_insert(ty);
            }
        }
        let mut uninferred = Vec::new();
        for tp in &fn_type_params {
            if mapping.contains_key(tp) {
                continue;
            }
            let declared_default = ft
                .type_params
                .iter()
                .position(|n| n == tp)
                .and_then(|i| ft.type_param_defaults.get(i).copied().flatten())
                .map(Type::resolved);
            if declared_default.is_none() && signature_mentions(ft, tp, checker, bind) {
                uninferred.push(tp.clone());
            }
            mapping.insert(tp.clone(), declared_default.unwrap_or(Type::Error));
        }
        (mapping, uninferred)
    } else {
        (
            fn_type_params
                .iter()
                .map(|k| (k.clone(), Type::Error))
                .collect(),
            Vec::new(),
        )
    }
}

pub(crate) fn infer_mapping_from_args(
    type_params: &[Arc<str>],
    param_types: &[FunctionParam],
    args: &[Arg],
    checker: &mut Checker,
    rec: &mut Recorder,
    bind: &BindResult,
) -> FxHashMap<Arc<str>, Type> {
    let mut mapping = FxHashMap::default();

    let mut taken = vec![false; param_types.len()];
    for arg in args.iter() {
        if let Arg::Named { label, .. } = arg
            && let Some(idx) = param_types
                .iter()
                .position(|p| p.name.as_deref() == Some(label.as_str()))
        {
            taken[idx] = true;
        }
    }
    let mut aligned: Vec<(&FunctionParam, &Arg)> = Vec::new();
    let mut positional_idx = 0usize;
    for arg in args.iter() {
        match arg {
            Arg::Named { label, .. } => {
                if let Some(idx) = param_types
                    .iter()
                    .position(|p| p.name.as_deref() == Some(label.as_str()))
                {
                    aligned.push((&param_types[idx], arg));
                }
            }
            Arg::Positional(_) => {
                while positional_idx < param_types.len() && taken[positional_idx] {
                    positional_idx += 1;
                }
                if let Some(param) = param_types.get(positional_idx) {
                    aligned.push((param, arg));
                }
                positional_idx += 1;
            }
            Arg::Spread(_) => {}
        }
    }

    for (param, arg) in aligned.iter().copied() {
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
            Arg::Positional(e) => checker.infer_type(rec, *e, bind),
            Arg::Named { value, .. } => checker.infer_type(rec, *value, bind),
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

    for (param, arg) in aligned.iter().copied() {
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
            Arg::Positional(e) | Arg::Named { value: e, .. } => {
                let mapped_kind = checker.ty_table.get(mapped_param_ty.0);
                if let TypeKind::Fn(fid) = mapped_kind {
                    let expected_fn = checker.ty_table.get_function(fid).clone();
                    if let Some(concrete) =
                        infer_arrow_with_context(*e, &expected_fn, checker, rec, bind)
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
                checker.infer_type(rec, *e, bind)
            }
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
    rec: &mut Recorder,
    bind: &BindResult,
) -> Option<Type> {
    let ExprKind::Arrow {
        params,
        return_type,
        body,
        is_async,
    } = checker.ast_arena.expr(expr).kind.clone()
    else {
        return None;
    };
    let expected = Type::fn_(
        expected_fn.clone(),
        &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
    );
    Some(checker.with_expected(Some(expected), |c| {
        c.infer_arrow_type(rec, expr, &params, &return_type, *body, is_async, bind)
    }))
}

pub(crate) fn find_arrow_scope(
    current_scope: vn_sem::scope::ScopeId,
    params: &[Param],
    bind: &BindResult,
) -> Option<vn_sem::scope::ScopeId> {
    if params.is_empty() {
        return None;
    }
    let param_names: Vec<&str> = params
        .iter()
        .flat_map(|p| p.pattern.bound_names())
        .map(|atom| bind.interner.resolve(atom))
        .filter(|name| *name != vn_core::TokenKind::Placeholder.as_str())
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
