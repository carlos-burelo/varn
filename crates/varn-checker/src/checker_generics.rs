use crate::binder::{pattern_lead_name, BindResult};
use crate::checker::Checker;
use crate::symbol::SymbolKind;
use crate::types::{FunctionParam, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Arg, ArrowBody, Expr, ExprKind, Stmt, StmtKind, TypeNode};
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
        infer_mapping_from_args(&fn_type_params, &ft.params, args, checker, bind)
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
        let arg_ty = match arg {
            Arg::Positional(e) => {
                if let TypeKind::Fn(expected_fn) = &param.ty.0 {
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
        actual_params.push(FunctionParam {
            name: Some(Rc::from(pattern_lead_name(&ap.pattern))),
            ty: ep.ty.clone(),
            optional: ap.is_optional,
            is_rest: ap.is_rest,
        });
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
            collect_returns(s, &mut returns);
            if returns.is_empty() {
                Type::Void
            } else if returns.len() == 1 {
                returns.pop().unwrap()
            } else {
                Type::union(returns)
            }
        }
    };

    Some(Type::fn_(FunctionType {
        params: actual_params,
        return_type: Box::new(ret_ty),
        is_arrow: true,
        type_params: Vec::new(),
    }))
}

fn collect_returns(stmt: &Stmt, out: &mut Vec<Type>) {
    match &stmt.kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_returns(s, out);
            }
        }
        StmtKind::Return { .. } => {}
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            collect_returns(consequent, out);
            if let Some(alt) = alternate {
                collect_returns(alt, out);
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
        _ => {}
    }
}
