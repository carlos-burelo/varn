//! Single-expression function inlining.
//!
//! A module function whose body is exactly `return <expr>` — no captures, no
//! `this`, no self-reference, no error propagation (`?`), no nested function
//! definitions — can be substituted directly at call sites whose arguments
//! are pure (variable reads or literals). Pure arguments make substitution
//! order-insensitive, so evaluation semantics (including short-circuiting
//! inside the body and unused parameters) are preserved exactly.
//!
//! Call targets must be module globals that are never reassigned anywhere in
//! the module; a single pass inlines one level (bodies containing calls are
//! themselves eligible, but inlined text is not re-scanned).

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    HirArrayEl, HirAssignTarget, HirBinding, HirExpr, HirFunction, HirModule, HirObjectProp,
    HirOptionalProperty, HirPropKey, HirStmt, HirTemplatePart, LocalId,
};

pub fn run(module: &mut HirModule) {
    let mutated = collect_mutated_globals(module);

    // Call sites name a locally-declared function by its qualified global
    // (`<source_file>::<name>`, built in `lower::global_binding`), never by
    // the bare `HirFunction::name`. Keying candidates on the bare name meant
    // the lookup in `rewrite_expr` could not match and nothing was ever
    // inlined. Qualifying here — rather than comparing suffixes at the call
    // site — also keeps `other.vn::helper` from colliding with a same-named
    // local one.
    let qualified = |name: &Rc<str>| -> Rc<str> {
        Rc::from(format!("{}::{}", module.source_file, name))
    };

    let mut candidates: Candidates = FxHashMap::default();
    for f in &module.functions {
        if f.is_async
            || f.is_generator
            || f.has_rest
            || f.has_this
            || f.upvalue_count != 0
            || f.params.iter().any(|p| p.default.is_some())
        {
            continue;
        }
        let global = qualified(&f.name);
        // Reassignment is recorded under the same qualified name the
        // assignment statement carries.
        if mutated.contains(&global) || mutated.contains(&f.name) {
            continue;
        }
        let Some(expr) = single_expression_body(&f.body) else {
            continue;
        };
        if body_is_inlinable(&expr) {
            candidates.insert(global, (f.params.len(), expr));
        }
    }
    if candidates.is_empty() {
        return;
    }

    let names: Vec<Rc<str>> = candidates.keys().cloned().collect();
    for name in names {
        // A candidate must not be inlined into itself (directly or via a
        // chain); dropping self-referential bodies keeps one pass terminating.
        if let Some((_, body)) = candidates.get(&name) {
            if body_mentions(body, &name) {
                candidates.remove(&name);
            }
        }
    }

    rewrite_function(&mut module.top_level, &candidates);
    for f in &mut module.functions {
        rewrite_function(f, &candidates);
    }
}

type Candidates = FxHashMap<Rc<str>, (usize, HirExpr)>;

/// Collapses a body to the single expression it returns, folding away any
/// straight-line `let` bindings on the way.
///
/// `return a * 2 + b` is directly one expression. `const t = a * 2; return t + b`
/// is the same value written in two steps, and the two shapes should not
/// optimize differently — but a `let` binds a `LocalId` that only means
/// anything inside the callee's frame, so the body is only substitutable once
/// every local is gone.
///
/// Folding is right-to-left: the last binding is substituted into the return
/// expression, then the one before it into what remains, so a binding may
/// refer to earlier ones. A local is only folded when it is read **at most
/// once**, unless its initializer is a bare literal or parameter. Without that
/// rule, `const t = expensive(); return t + t` would inline into two calls and
/// the "optimization" would double the work.
fn single_expression_body(body: &[HirStmt]) -> Option<HirExpr> {
    let (last, leading) = body.split_last()?;
    let HirStmt::Return(Some(result)) = last else {
        return None;
    };

    let mut bindings = Vec::with_capacity(leading.len());
    for stmt in leading {
        match stmt {
            HirStmt::Let { local, value, .. } => bindings.push((*local, value.clone())),
            // Anything else is a statement with its own control flow or
            // effect, which this substitution cannot represent.
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
            // An unread binding whose initializer still does something has to
            // keep happening; there is nowhere to put it in an expression.
            return None;
        }
    }
    Some(folded)
}

/// Safe to substitute more than once: re-evaluating costs nothing and
/// observes nothing.
fn is_duplicable(e: &HirExpr) -> bool {
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
fn init_is_droppable(e: &HirExpr) -> bool {
    is_duplicable(e) || matches!(e, HirExpr::Var(_))
}

/// Takes `&mut` only to reuse the single traversal helper — it does not
/// modify anything. Adding an immutable twin would mean a second copy of the
/// per-variant child list, which is exactly the duplication that made these
/// traversals drift apart before.
fn count_local_reads(e: &mut HirExpr, local: LocalId) -> usize {
    if matches!(e, HirExpr::Var(HirBinding::Local(l)) if *l == local) {
        return 1;
    }
    let mut n = 0;
    for_each_child_expr_mut(e, &mut |c| n += count_local_reads(c, local));
    n
}

fn substitute_local(e: &mut HirExpr, local: LocalId, init: &HirExpr) {
    if matches!(e, HirExpr::Var(HirBinding::Local(l)) if *l == local) {
        *e = init.clone();
        return;
    }
    for_each_child_expr_mut(e, &mut |c| substitute_local(c, local, init));
}

// ---- candidate body validation -------------------------------------------

/// Whitelist of expressions that behave identically when moved from the
/// callee into a caller: no function-relative control flow (`return`-like
/// `?`, `yield`), no environment capture, no self/`this`/`super` reference,
/// no nested function definitions, no mutation of caller-visible bindings.
fn body_is_inlinable(e: &HirExpr) -> bool {
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
        Array(els) => els.iter().all(|el| match el {
            HirArrayEl::Expr(x) | HirArrayEl::Spread(x) => body_is_inlinable(x),
            HirArrayEl::Hole => true,
        }),
        Object { properties } => properties.iter().all(|p| match p {
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

        // Function-relative or environment-capturing constructs.
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

fn body_mentions(e: &HirExpr, name: &Rc<str>) -> bool {
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

/// Immutable walk over an expression tree (does not cross into nested
/// `HirFunction` bodies — validated candidate bodies contain none).
pub(crate) fn walk_exprs<'a>(e: &'a HirExpr, f: &mut impl FnMut(&'a HirExpr)) {
    f(e);
    // Clone-free traversal via the mutable-children helper is not possible on
    // a shared reference; mirror the child list instead.
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
        | Regex { .. }
        | Var(_)
        | This
        | Super
        | SuperMember { .. } => {}
        NonNull(x)
        | TryOp(x)
        | Spread(x)
        | Await(x)
        | Spawn(x)
        | Yield(x)
        | TypeTest { value: x, .. } => walk_exprs(x, f),
        Sequence(xs)
        | SelfCall { args: xs, .. }
        | SuperCall { args: xs }
        | SuperMethodCall { args: xs, .. } => {
            for x in xs {
                walk_exprs(x, f);
            }
        }
        Range { start, end, .. } => {
            walk_exprs(start, f);
            walk_exprs(end, f);
        }
        Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(x) = p {
                    walk_exprs(x, f);
                }
            }
        }
        Assign { value, .. } => walk_exprs(value, f),
        Update { .. } => {}
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            walk_exprs(lhs, f);
            walk_exprs(rhs, f);
        }
        Unary { operand, .. } => walk_exprs(operand, f),
        Call { callee, args, .. } => {
            walk_exprs(callee, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => walk_exprs(object, f),
        Index { object, index, .. } => {
            walk_exprs(object, f);
            walk_exprs(index, f);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            walk_exprs(recv, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            walk_exprs(object, f);
            for x in args {
                walk_exprs(x, f);
            }
        }
        Conditional { test, cons, alt } => {
            walk_exprs(test, f);
            walk_exprs(cons, f);
            walk_exprs(alt, f);
        }
        Array(els) => {
            for el in els {
                if let HirArrayEl::Expr(x) | HirArrayEl::Spread(x) = el {
                    walk_exprs(x, f);
                }
            }
        }
        Object { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(x) = key {
                            walk_exprs(x, f);
                        }
                        walk_exprs(value, f);
                    }
                    HirObjectProp::Spread(x) => walk_exprs(x, f),
                    HirObjectProp::Method { .. } => {}
                }
            }
        }
        OptionalChain { object, property } => {
            walk_exprs(object, f);
            match property {
                HirOptionalProperty::Member(_)
                | HirOptionalProperty::ModuleSlot(_)
                | HirOptionalProperty::Extension(_) => {}
                HirOptionalProperty::Index(x) => walk_exprs(x, f),
                HirOptionalProperty::Call(args)
                | HirOptionalProperty::MethodCall(_, args)
                | HirOptionalProperty::ExtensionCall(_, args) => {
                    for x in args {
                        walk_exprs(x, f);
                    }
                }
            }
        }
        TaggedTemplate { tag, template } => {
            walk_exprs(tag, f);
            walk_exprs(template, f);
        }
        Closure { .. } | Class(_) | Enum(_) | Match { .. } => {}
    }
}

// ---- reassigned-global collection ----------------------------------------

fn collect_mutated_globals(module: &HirModule) -> FxHashSet<Rc<str>> {
    let mut out = FxHashSet::default();
    scan_function(&module.top_level, &mut out);
    for f in &module.functions {
        scan_function(f, &mut out);
    }
    out
}

fn scan_function(f: &HirFunction, out: &mut FxHashSet<Rc<str>>) {
    scan_stmts(&f.body, out);
}

fn scan_stmts(stmts: &[HirStmt], out: &mut FxHashSet<Rc<str>>) {
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

// ---- call-site rewriting ---------------------------------------------------

fn rewrite_function(f: &mut HirFunction, candidates: &Candidates) {
    for s in &mut f.body {
        rewrite_stmt(s, candidates);
    }
}

fn rewrite_stmt(s: &mut HirStmt, candidates: &Candidates) {
    for_each_stmt_expr_mut(s, &mut |e| rewrite_expr(e, candidates));
    for child in child_stmts_mut(s) {
        rewrite_stmt(child, candidates);
    }
}

fn rewrite_expr(e: &mut HirExpr, candidates: &Candidates) {
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
fn arg_is_pure(e: &HirExpr) -> bool {
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

fn substitute_params(e: &mut HirExpr, args: &[HirExpr]) {
    if let HirExpr::Var(HirBinding::Param(i)) = e {
        if let Some(arg) = args.get(*i as usize) {
            *e = arg.clone();
            return;
        }
    }
    for_each_child_expr_mut(e, &mut |c| substitute_params(c, args));
}

// ---- generic traversal helpers ---------------------------------------------

/// Apply `f` to every direct child expression of `e`. Does not descend into
/// nested `HirFunction` bodies (closures, classes, enums, object methods).
fn for_each_child_expr_mut(e: &mut HirExpr, f: &mut impl FnMut(&mut HirExpr)) {
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
        | Regex { .. }
        | Var(_)
        | This
        | Super
        | SuperMember { .. } => {}
        NonNull(x)
        | TryOp(x)
        | Spread(x)
        | Await(x)
        | Spawn(x)
        | Yield(x)
        | TypeTest { value: x, .. } => f(x),
        Sequence(xs)
        | SelfCall { args: xs, .. }
        | SuperCall { args: xs }
        | SuperMethodCall { args: xs, .. } => {
            for x in xs {
                f(x);
            }
        }
        Range { start, end, .. } => {
            f(start);
            f(end);
        }
        Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(x) = p {
                    f(x);
                }
            }
        }
        Assign { target, value } => {
            for_each_assign_target_expr_mut(target, f);
            f(value);
        }
        Update { target, .. } => for_each_assign_target_expr_mut(target, f),
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            f(lhs);
            f(rhs);
        }
        Unary { operand, .. } => f(operand),
        Call { callee, args, .. } => {
            f(callee);
            for x in args {
                f(x);
            }
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => f(object),
        Index { object, index, .. } => {
            f(object);
            f(index);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            f(recv);
            for x in args {
                f(x);
            }
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            f(object);
            for x in args {
                f(x);
            }
        }
        Conditional { test, cons, alt } => {
            f(test);
            f(cons);
            f(alt);
        }
        Array(els) => {
            for el in els {
                if let HirArrayEl::Expr(x) | HirArrayEl::Spread(x) = el {
                    f(x);
                }
            }
        }
        Object { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(x) = key {
                            f(x);
                        }
                        f(value);
                    }
                    HirObjectProp::Spread(x) => f(x),
                    HirObjectProp::Method { .. } => {}
                }
            }
        }
        OptionalChain { object, property } => {
            f(object);
            match property {
                HirOptionalProperty::Member(_)
                | HirOptionalProperty::ModuleSlot(_)
                | HirOptionalProperty::Extension(_) => {}
                HirOptionalProperty::Index(x) => f(x),
                HirOptionalProperty::Call(args)
                | HirOptionalProperty::MethodCall(_, args)
                | HirOptionalProperty::ExtensionCall(_, args) => {
                    for x in args {
                        f(x);
                    }
                }
            }
        }
        TaggedTemplate { tag, template } => {
            f(tag);
            f(template);
        }
        Match { subject, cases } => {
            f(subject);
            for c in cases {
                if let Some(g) = &mut c.guard {
                    f(g);
                }
                if let Some(r) = &mut c.result {
                    f(r);
                }
            }
        }
        Closure { .. } | Class(_) | Enum(_) => {}
    }
}

fn for_each_assign_target_expr_mut(t: &mut HirAssignTarget, f: &mut impl FnMut(&mut HirExpr)) {
    match t {
        HirAssignTarget::Var(_)
        | HirAssignTarget::ModuleSlot { .. }
        | HirAssignTarget::SuperMember { .. } => {}
        HirAssignTarget::Member { object, .. } | HirAssignTarget::SetFixedField { object, .. } => {
            f(object)
        }
        HirAssignTarget::Index { object, index, .. } => {
            f(object);
            f(index);
        }
        HirAssignTarget::SuperIndex { index } => f(index),
    }
}

/// Apply `f` to every expression directly owned by `s` (not those in child
/// statements).
fn for_each_stmt_expr_mut(s: &mut HirStmt, f: &mut impl FnMut(&mut HirExpr)) {
    match s {
        HirStmt::Expr(e)
        | HirStmt::Let { value: e, .. }
        | HirStmt::Assign { value: e, .. }
        | HirStmt::Return(Some(e))
        | HirStmt::Throw(e)
        | HirStmt::ExportDefaultExpr { value: e, .. }
        | HirStmt::While { test: e, .. }
        | HirStmt::DoWhile { test: e, .. }
        | HirStmt::ForClassic { test: e, .. }
        | HirStmt::ForOf { iterable: e, .. }
        | HirStmt::ForIn { object: e, .. }
        | HirStmt::If { test: e, .. }
        | HirStmt::Switch { disc: e, .. } => f(e),
        HirStmt::SetMember { object, value, .. }
        | HirStmt::SetFixedField { object, value, .. } => {
            f(object);
            f(value);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            f(object);
            f(index);
            f(value);
        }
        HirStmt::Return(None)
        | HirStmt::Break
        | HirStmt::Continue
        | HirStmt::CloseUpvalues(_)
        | HirStmt::Import { .. }
        | HirStmt::StoreExport { .. }
        | HirStmt::ExportNamed { .. }
        | HirStmt::ExportAll { .. }
        | HirStmt::Try { .. }
        | HirStmt::Dispose { .. } => {}
    }
    if let HirStmt::Switch { cases, .. } = s {
        for c in cases {
            if let Some(t) = &mut c.test {
                f(t);
            }
        }
    }
}

/// Immutable variant used by the global-mutation scan.
pub(crate) fn for_each_stmt_expr<'a>(s: &'a HirStmt, f: &mut impl FnMut(&'a HirExpr)) {
    let mut apply = |e: &'a HirExpr| walk_exprs(e, f);
    match s {
        HirStmt::Expr(e)
        | HirStmt::Let { value: e, .. }
        | HirStmt::Assign { value: e, .. }
        | HirStmt::Return(Some(e))
        | HirStmt::Throw(e)
        | HirStmt::ExportDefaultExpr { value: e, .. }
        | HirStmt::While { test: e, .. }
        | HirStmt::DoWhile { test: e, .. }
        | HirStmt::ForClassic { test: e, .. }
        | HirStmt::ForOf { iterable: e, .. }
        | HirStmt::ForIn { object: e, .. }
        | HirStmt::If { test: e, .. }
        | HirStmt::Switch { disc: e, .. } => apply(e),
        HirStmt::SetMember { object, value, .. }
        | HirStmt::SetFixedField { object, value, .. } => {
            apply(object);
            apply(value);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            apply(object);
            apply(index);
            apply(value);
        }
        HirStmt::Return(None)
        | HirStmt::Break
        | HirStmt::Continue
        | HirStmt::CloseUpvalues(_)
        | HirStmt::Import { .. }
        | HirStmt::StoreExport { .. }
        | HirStmt::ExportNamed { .. }
        | HirStmt::ExportAll { .. }
        | HirStmt::Try { .. }
        | HirStmt::Dispose { .. } => {}
    }
    if let HirStmt::Switch { cases, .. } = s {
        for c in cases {
            if let Some(t) = &c.test {
                apply(t);
            }
        }
    }
}

fn child_stmts_mut(s: &mut HirStmt) -> Vec<&mut HirStmt> {
    match s {
        HirStmt::If {
            then_body,
            else_body,
            ..
        } => then_body.iter_mut().chain(else_body.iter_mut()).collect(),
        HirStmt::While { body, .. }
        | HirStmt::ForOf { body, .. }
        | HirStmt::ForIn { body, .. }
        | HirStmt::DoWhile { body, .. } => body.iter_mut().collect(),
        HirStmt::ForClassic { update, body, .. } => {
            update.iter_mut().chain(body.iter_mut()).collect()
        }
        HirStmt::Switch { cases, .. } => cases.iter_mut().flat_map(|c| c.body.iter_mut()).collect(),
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            let mut v: Vec<&mut HirStmt> = block.iter_mut().collect();
            if let Some(c) = catch {
                v.extend(c.body.iter_mut());
            }
            if let Some(fin) = finally {
                v.extend(fin.iter_mut());
            }
            v
        }
        _ => Vec::new(),
    }
}

pub(crate) fn push_child_stmts<'a>(s: &'a HirStmt, out: &mut Vec<&'a HirStmt>) {
    match s {
        HirStmt::If {
            then_body,
            else_body,
            ..
        } => {
            out.extend(then_body.iter());
            out.extend(else_body.iter());
        }
        HirStmt::While { body, .. }
        | HirStmt::ForOf { body, .. }
        | HirStmt::ForIn { body, .. }
        | HirStmt::DoWhile { body, .. } => out.extend(body.iter()),
        HirStmt::ForClassic { update, body, .. } => {
            out.extend(update.iter());
            out.extend(body.iter());
        }
        HirStmt::Switch { cases, .. } => {
            for c in cases {
                out.extend(c.body.iter());
            }
        }
        HirStmt::Try {
            block,
            catch,
            finally,
        } => {
            out.extend(block.iter());
            if let Some(c) = catch {
                out.extend(c.body.iter());
            }
            if let Some(fin) = finally {
                out.extend(fin.iter());
            }
        }
        _ => {}
    }
}
