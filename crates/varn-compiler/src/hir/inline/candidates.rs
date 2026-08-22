use std::rc::Rc;
use rustc_hash::FxHashSet;

use crate::hir::{
    HirArrayEl, HirAssignTarget, HirBinding, HirExpr, HirFunction, HirModule, HirObjectProp,
    HirOptionalProperty, HirPropKey, HirStmt, HirTemplatePart, LocalId,
};
use super::traverse::{
    for_each_child_expr_mut, for_each_stmt_expr, push_child_stmts, walk_exprs,
};

/// Collapses a body to the single expression it returns, folding away any
/// straight-line `let` bindings on the way.
pub(crate) fn single_expression_body(body: &[HirStmt]) -> Option<HirExpr> {
    let (last, leading) = body.split_last()?;
    let HirStmt::Return(Some(result)) = last else {
        return None;
    };

    let mut bindings = Vec::with_capacity(leading.len());
    for stmt in leading {
        match stmt {
            HirStmt::Let { local, value, .. } => bindings.push((*local, value.clone())),
            _ => return None,
        }
    }

    let mut folded = result.clone();
    for (local, init) in bindings.into_iter().rev() {
        let reads = count_local_reads(&mut folded, local);
        if reads > 1 && !is_duplicable(&init) {
            return None;
        }
        if reads > 0 {
            substitute_local(&mut folded, local, &init);
        } else if !init_is_droppable(&init) {
            return None;
        }
    }
    Some(folded)
}

/// Safe to substitute more than once: re-evaluating costs nothing and
/// observes nothing.
pub(crate) fn is_duplicable(e: &HirExpr) -> bool {
    matches!(
        e,
        HirExpr::Int(_)
            | HirExpr::Float(_)
            | HirExpr::Str(_)
            | HirExpr::Bool(_)
            | HirExpr::Char(_)
            | HirExpr::Decimal(_)
            | HirExpr::BigInt(_)
            | HirExpr::Null
            | HirExpr::Var(HirBinding::Param(_))
    )
}

/// Safe to discard entirely, because nothing observes that it ran.
pub(crate) fn init_is_droppable(e: &HirExpr) -> bool {
    is_duplicable(e) || matches!(e, HirExpr::Var(_))
}

pub(crate) fn count_local_reads(e: &mut HirExpr, local: LocalId) -> usize {
    if matches!(e, HirExpr::Var(HirBinding::Local(l)) if *l == local) {
        return 1;
    }
    let mut n = 0;
    for_each_child_expr_mut(e, &mut |c| n += count_local_reads(c, local));
    n
}

pub(crate) fn substitute_local(e: &mut HirExpr, local: LocalId, init: &HirExpr) {
    if matches!(e, HirExpr::Var(HirBinding::Local(l)) if *l == local) {
        *e = init.clone();
        return;
    }
    for_each_child_expr_mut(e, &mut |c| substitute_local(c, local, init));
}

/// Whitelist of expressions that behave identically when moved from the
/// callee into a caller.
pub(crate) fn body_is_inlinable(e: &HirExpr) -> bool {
    use HirExpr::*;
    match e {
        Int(_)
        | Float(_)
        | Str(_)
        | Bool(_)
        | Char(_)
        | Decimal(_)
        | BigInt(_)
        | Null
        | Regex { .. } => true,
        Var(HirBinding::Param(_)) | Var(HirBinding::Global(_)) => true,
        Var(HirBinding::Local(_)) | Var(HirBinding::Upvalue(_)) => false,

        NonNull(x) | Spread(x) | TypeTest { value: x, .. } => body_is_inlinable(x),
        Sequence(xs) => xs.iter().all(body_is_inlinable),
        Range { start, end, .. } => body_is_inlinable(start) && body_is_inlinable(end),
        Template(parts) => parts.iter().all(|p| match p {
            HirTemplatePart::Str(_) => true,
            HirTemplatePart::Expr(x) => body_is_inlinable(x),
        }),
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            body_is_inlinable(lhs) && body_is_inlinable(rhs)
        }
        Unary { operand, .. } => body_is_inlinable(operand),
        Call { callee, args, .. } => {
            body_is_inlinable(callee) && args.iter().all(body_is_inlinable)
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => body_is_inlinable(object),
        Index { object, index, .. } => body_is_inlinable(object) && body_is_inlinable(index),
        MethodCall { recv, args, .. } => {
            body_is_inlinable(recv) && args.iter().all(body_is_inlinable)
        }
        ExtensionCall { recv, args, .. } => {
            body_is_inlinable(recv) && args.iter().all(body_is_inlinable)
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            body_is_inlinable(object) && args.iter().all(body_is_inlinable)
        }
        Conditional { test, cons, alt } => {
            body_is_inlinable(test) && body_is_inlinable(cons) && body_is_inlinable(alt)
        }
        Array(els) | Tuple(els) => els.iter().all(|el| match el {
            HirArrayEl::Expr(x) | HirArrayEl::Spread(x) => body_is_inlinable(x),
            HirArrayEl::Hole => true,
        }),
        Object { properties } | Record { properties } => properties.iter().all(|p| match p {
            HirObjectProp::Property { key, value } => {
                let key_ok = match key {
                    HirPropKey::Static(_) => true,
                    HirPropKey::Computed(x) => body_is_inlinable(x),
                };
                key_ok && body_is_inlinable(value)
            }
            HirObjectProp::Spread(x) => body_is_inlinable(x),
            HirObjectProp::Method { .. } => false,
        }),
        OptionalChain { object, property } => {
            body_is_inlinable(object)
                && match property {
                    HirOptionalProperty::Member(_)
                    | HirOptionalProperty::ModuleSlot(_)
                    | HirOptionalProperty::Extension(_) => true,
                    HirOptionalProperty::Index(x) => body_is_inlinable(x),
                    HirOptionalProperty::Call(args)
                    | HirOptionalProperty::MethodCall(_, args)
                    | HirOptionalProperty::ExtensionCall(_, args) => {
                        args.iter().all(body_is_inlinable)
                    }
                }
        }

        TryOp(_)
        | SelfCall { .. }
        | This
        | Super
        | SuperCall { .. }
        | SuperMethodCall { .. }
        | SuperMember { .. }
        | Closure { .. }
        | Class(_)
        | Enum(_)
        | Match { .. }
        | Await(_)
        | Spawn(_)
        | Yield(_)
        | TaggedTemplate { .. }
        | Assign { .. }
        | Update { .. } => false,
    }
}

pub(crate) fn body_mentions(e: &HirExpr, name: &Rc<str>) -> bool {
    let mut found = false;
    walk_exprs(e, &mut |x| {
        if let HirExpr::Var(HirBinding::Global(g)) = x {
            if g == name {
                found = true;
            }
        }
    });
    found
}

pub(crate) fn collect_mutated_globals(module: &HirModule) -> FxHashSet<Rc<str>> {
    let mut out = FxHashSet::default();
    scan_function(&module.top_level, &mut out);
    for f in &module.functions {
        scan_function(f, &mut out);
    }
    out
}

pub(crate) fn scan_function(f: &HirFunction, out: &mut FxHashSet<Rc<str>>) {
    scan_stmts(&f.body, out);
}

pub(crate) fn scan_stmts(stmts: &[HirStmt], out: &mut FxHashSet<Rc<str>>) {
    for s in stmts {
        if let HirStmt::Assign {
            target: HirBinding::Global(g),
            ..
        } = s
        {
            out.insert(g.clone());
        }
        for_each_stmt_expr(s, &mut |e| {
            let mut check = |t: &HirAssignTarget| {
                if let HirAssignTarget::Var(HirBinding::Global(g)) = t {
                    out.insert(g.clone());
                }
            };
            match e {
                HirExpr::Assign { target, .. } => check(target),
                HirExpr::Update { target, .. } => check(target),
                HirExpr::Closure { func, .. } => scan_function(func, out),
                HirExpr::Match { cases, .. } => {
                    for c in cases {
                        scan_stmts(&c.body, out);
                    }
                }
                HirExpr::Class(c) => {
                    scan_function(&c.ctor.func, out);
                    for m in c
                        .methods
                        .iter()
                        .chain(&c.static_methods)
                        .chain(&c.static_blocks)
                    {
                        scan_function(&m.func, out);
                    }
                    for a in c.getters.iter().chain(&c.setters) {
                        scan_function(&a.func, out);
                    }
                }
                HirExpr::Enum(en) => {
                    scan_function(&en.ctor.func, out);
                    for m in en
                        .methods
                        .iter()
                        .chain(&en.static_methods)
                        .chain(&en.static_blocks)
                    {
                        scan_function(&m.func, out);
                    }
                    for a in en.getters.iter().chain(&en.setters) {
                        scan_function(&a.func, out);
                    }
                }
                HirExpr::Object { properties } => {
                    for p in properties {
                        if let HirObjectProp::Method { func, .. } = p {
                            scan_function(func, out);
                        }
                    }
                }
                _ => {}
            }
        });
        let mut children: Vec<&HirStmt> = Vec::new();
        push_child_stmts(s, &mut children);
        for c in children {
            scan_stmts(std::slice::from_ref(c), out);
        }
    }
}
