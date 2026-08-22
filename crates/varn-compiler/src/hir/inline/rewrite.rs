use crate::hir::{
    HirBinding, HirExpr, HirFunction, HirObjectProp, HirStmt,
};
use super::traverse::{child_stmts_mut, for_each_child_expr_mut, for_each_stmt_expr_mut};
use super::Candidates;

pub(crate) fn rewrite_function(f: &mut HirFunction, candidates: &Candidates) {
    for s in &mut f.body {
        rewrite_stmt(s, candidates);
    }
}

pub(crate) fn rewrite_stmt(s: &mut HirStmt, candidates: &Candidates) {
    for_each_stmt_expr_mut(s, &mut |e| rewrite_expr(e, candidates));
    for child in child_stmts_mut(s) {
        rewrite_stmt(child, candidates);
    }
}

pub(crate) fn rewrite_expr(e: &mut HirExpr, candidates: &Candidates) {
    // Children first, so freshly substituted bodies are not re-visited.
    for_each_child_expr_mut(e, &mut |c| rewrite_expr(c, candidates));

    // Nested function definitions and statement bodies get their own rewrite.
    match e {
        HirExpr::Closure { func, .. } => rewrite_function(func, candidates),
        HirExpr::Match { cases, .. } => {
            for c in cases {
                for s in &mut c.body {
                    rewrite_stmt(s, candidates);
                }
            }
        }
        HirExpr::Class(c) => {
            rewrite_function(&mut c.ctor.func, candidates);
            for m in c
                .methods
                .iter_mut()
                .chain(&mut c.static_methods)
                .chain(&mut c.static_blocks)
            {
                rewrite_function(&mut m.func, candidates);
            }
            for a in c.getters.iter_mut().chain(&mut c.setters) {
                rewrite_function(&mut a.func, candidates);
            }
        }
        HirExpr::Enum(en) => {
            rewrite_function(&mut en.ctor.func, candidates);
            for m in en
                .methods
                .iter_mut()
                .chain(&mut en.static_methods)
                .chain(&mut en.static_blocks)
            {
                rewrite_function(&mut m.func, candidates);
            }
            for a in en.getters.iter_mut().chain(&mut en.setters) {
                rewrite_function(&mut a.func, candidates);
            }
        }
        HirExpr::Object { properties } => {
            for p in properties {
                if let HirObjectProp::Method { func, .. } = p {
                    rewrite_function(func, candidates);
                }
            }
        }
        _ => {}
    }

    let HirExpr::Call { callee, args, .. } = e else {
        return;
    };
    let HirExpr::Var(HirBinding::Global(name)) = callee.as_ref() else {
        return;
    };
    let Some((arity, body)) = candidates.get(name) else {
        return;
    };
    if args.len() != *arity || !args.iter().all(arg_is_pure) {
        return;
    }

    let mut inlined = body.clone();
    substitute_params(&mut inlined, args);
    *e = inlined;
}

/// Arguments whose evaluation has no observable effect and is order-free.
pub(crate) fn arg_is_pure(e: &HirExpr) -> bool {
    matches!(
        e,
        HirExpr::Var(_)
            | HirExpr::Int(_)
            | HirExpr::Float(_)
            | HirExpr::Str(_)
            | HirExpr::Bool(_)
            | HirExpr::Char(_)
            | HirExpr::Decimal(_)
            | HirExpr::BigInt(_)
            | HirExpr::Null
    )
}

pub(crate) fn substitute_params(e: &mut HirExpr, args: &[HirExpr]) {
    if let HirExpr::Var(HirBinding::Param(i)) = e {
        if let Some(arg) = args.get(*i as usize) {
            *e = arg.clone();
            return;
        }
    }
    for_each_child_expr_mut(e, &mut |c| substitute_params(c, args));
}
