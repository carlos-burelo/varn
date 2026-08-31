//! Promoting module-private top-level `let`/`const` to `<module>` registers.
//!
//! A top-level binding is a GLOBAL by default: `Scope::is_global()` is true
//! for the module frame, so `let n = 150` compiles to a global slot and every
//! read is two dependent loads (`ExecCtx` → the values pointer → the slot)
//! plus unbox shifts, with a store back on every write. As a register it is
//! just a register. Measured by hand-wrapping the same code in a function:
//! ~2x on `bench_matrix`, `bench_array_ops`, `bench_dto` and a bare 5M-iteration
//! loop.
//!
//! # Why this cannot simply promote everything
//!
//! Promotion is only sound for a binding whose every read happens in the
//! module's OWN frame. Four things take it out of that frame, and each one is
//! a correctness requirement, not a heuristic:
//!
//! - **Top-level functions** are lowered with a fresh `Scope::new()`, so
//!   `resolve_upvalue` returns `None` at once and anything they reference
//!   falls through to a global. A promoted binding would simply not be found.
//! - **Closures, classes and enums** likewise read globals directly rather
//!   than capturing them.
//! - **Exports** are read by other modules through their slot.
//! - **Namespace members** are declared as globals and read back to build the
//!   namespace object.
//!
//! The last two are filtered at the declaration site (`lower::decl`, which is
//! the only place that knows a global write is a declaration and not an
//! assignment). The first two are what [`nested_globals`] answers.
//!
//! # The walker is exhaustive on purpose
//!
//! [`walk_stmts`] matches every `HirStmt` and `HirExpr` variant with no `_`
//! arm. That is deliberate: a container added to the HIR and missed here
//! would silently hide a use, and this pass would promote a global that a
//! closure still reads — a wrong-code bug with no compile error and no failing
//! test. With the match exhaustive, adding a variant breaks the build instead.
//! (The same catch-all mistake in `checker_annotations` cost real performance
//! three separate times; here it would cost correctness.)

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    HirArrayEl, HirAssignTarget, HirBinding, HirCatch, HirExpr, HirFunction, HirModule,
    HirObjectProp, HirOptionalProperty, HirPropKey, HirStmt, HirTemplatePart, LocalId,
};

/// Cap on how many bindings one module may promote. Every promotion adds a
/// register to the module frame, and `ssa::emit` hard-errors past 255 — a
/// program that compiles today must not stop compiling because of an
/// optimization. The allocator reuses registers across disjoint live ranges,
/// so this is far more headroom than it looks.
const MAX_PROMOTED: usize = 64;

pub fn run(module: &mut HirModule) {
    if module.top_level_lets.is_empty() {
        return;
    }
    let blocked = nested_globals(module);

    let mut promoted: FxHashMap<Rc<str>, LocalId> = FxHashMap::default();
    let mut next_local = module.top_level.locals;
    for name in &module.top_level_lets {
        if promoted.len() >= MAX_PROMOTED {
            break;
        }
        if blocked.contains(name) || promoted.contains_key(name) {
            continue;
        }
        promoted.insert(name.clone(), LocalId(next_local));
        next_local += 1;
    }
    if promoted.is_empty() {
        return;
    }
    module.top_level.locals = next_local;

    // Rewrite only the module's own frame; `in_nested` guards the rest.
    let mut body = std::mem::take(&mut module.top_level.body);
    walk_stmts(&mut body, false, &mut |b, in_nested| {
        if in_nested {
            return;
        }
        if let HirBinding::Global(g, _) = b {
            if let Some(&local) = promoted.get(g) {
                *b = HirBinding::Local(local);
            }
        }
    });
    // A promoted declaration was `Assign { target: Global }`; the rewrite above
    // turned it into `Assign { target: Local }`, which is exactly the store the
    // backend wants for a local. No `Let` conversion is needed: the module
    // frame's registers are zero-initialized and the declaration dominates
    // every read (the checker rejects use-before-declaration).
    module.top_level.body = body;
}

/// Every global named from outside the module's own frame.
fn nested_globals(module: &HirModule) -> FxHashSet<Rc<str>> {
    let mut out = FxHashSet::default();
    let collect = |f: &HirFunction, start_nested: bool, out: &mut FxHashSet<Rc<str>>| {
        let mut body = f.body.clone();
        walk_stmts(&mut body, start_nested, &mut |b, in_nested| {
            if in_nested {
                if let HirBinding::Global(g, _) = b {
                    out.insert(g.clone());
                }
            }
        });
    };
    // Module functions run in their own frame: everything they name counts.
    for f in &module.functions {
        collect(f, true, &mut out);
    }
    // In the top level, only what sits inside a nested body counts.
    collect(&module.top_level, false, &mut out);
    out
}

fn walk_stmts(stmts: &mut [HirStmt], in_nested: bool, act: &mut impl FnMut(&mut HirBinding, bool)) {
    for s in stmts {
        walk_stmt(s, in_nested, act);
    }
}

fn walk_stmt(s: &mut HirStmt, n: bool, act: &mut impl FnMut(&mut HirBinding, bool)) {
    match s {
        HirStmt::Expr(e) | HirStmt::Throw(e) => walk_expr(e, n, act),
        HirStmt::Let { value, .. } => walk_expr(value, n, act),
        HirStmt::Assign { target, value } => {
            act(target, n);
            walk_expr(value, n, act);
        }
        HirStmt::SetMember { object, value, .. } => {
            walk_expr(object, n, act);
            walk_expr(value, n, act);
        }
        HirStmt::SetFixedField { object, value, .. } => {
            walk_expr(object, n, act);
            walk_expr(value, n, act);
        }
        HirStmt::SetIndex {
            object,
            index,
            value,
            ..
        } => {
            walk_expr(object, n, act);
            walk_expr(index, n, act);
            walk_expr(value, n, act);
        }
        HirStmt::Return(v) => {
            if let Some(e) = v {
                walk_expr(e, n, act);
            }
        }
        HirStmt::If {
            test,
            then_body,
            else_body,
        } => {
            walk_expr(test, n, act);
            walk_stmts(then_body, n, act);
            walk_stmts(else_body, n, act);
        }
        HirStmt::While { test, body } | HirStmt::DoWhile { body, test } => {
            walk_expr(test, n, act);
            walk_stmts(body, n, act);
        }
        HirStmt::ForClassic { test, update, body } => {
            walk_expr(test, n, act);
            walk_stmts(update, n, act);
            walk_stmts(body, n, act);
        }
        HirStmt::ForOf { iterable, body, .. } => {
            walk_expr(iterable, n, act);
            walk_stmts(body, n, act);
        }
        HirStmt::ForIn { object, body, .. } => {
            walk_expr(object, n, act);
            walk_stmts(body, n, act);
        }
        HirStmt::Switch { disc, cases } => {
            walk_expr(disc, n, act);
            for c in cases {
                if let Some(t) = &mut c.test {
                    walk_expr(t, n, act);
                }
                walk_stmts(&mut c.body, n, act);
            }
        }
        HirStmt::Try {
            block,
            catches,
            finally,
        } => {
            walk_stmts(block, n, act);
            for HirCatch { body, .. } in catches {
                walk_stmts(body, n, act);
            }
            if let Some(f) = finally {
                walk_stmts(f, n, act);
            }
        }
        HirStmt::ExportDefaultExpr { value, .. } => walk_expr(value, n, act),
        HirStmt::Break
        | HirStmt::Continue
        | HirStmt::CloseUpvalues(_)
        | HirStmt::Import { .. }
        | HirStmt::StoreExport { .. }
        | HirStmt::ExportNamed { .. }
        | HirStmt::ExportAll { .. }
        // Names a local, never a global.
        | HirStmt::Dispose { .. }
        | HirStmt::Line(_) => {}
    }
}

fn walk_target(t: &mut HirAssignTarget, n: bool, act: &mut impl FnMut(&mut HirBinding, bool)) {
    match t {
        HirAssignTarget::Var(b) => act(b, n),
        HirAssignTarget::Member { object, .. } => walk_expr(object, n, act),
        HirAssignTarget::SetFixedField { object, .. } => walk_expr(object, n, act),
        HirAssignTarget::Index { object, index, .. } => {
            walk_expr(object, n, act);
            walk_expr(index, n, act);
        }
        HirAssignTarget::ModuleSlot { .. } | HirAssignTarget::SuperMember { .. } => {}
        HirAssignTarget::SuperIndex { index } => walk_expr(index, n, act),
    }
}

fn walk_expr(e: &mut HirExpr, n: bool, act: &mut impl FnMut(&mut HirBinding, bool)) {
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
        | This
        | Super
        | SuperMember { .. } => {}
        Var(b) => act(b, n),
        NonNull(x)
        | TryOp(x)
        | Spread(x)
        | Await(x)
        | Spawn(x)
        | Yield(x)
        | TypeTest { value: x, .. } => walk_expr(x, n, act),
        Sequence(xs)
        | SelfCall { args: xs, .. }
        | SuperCall { args: xs }
        | SuperMethodCall { args: xs, .. } => {
            for x in xs {
                walk_expr(x, n, act);
            }
        }
        Range { start, end, .. } => {
            walk_expr(start, n, act);
            walk_expr(end, n, act);
        }
        Template(parts) => {
            for p in parts {
                if let HirTemplatePart::Expr(x) = p {
                    walk_expr(x, n, act);
                }
            }
        }
        Assign { target, value } => {
            walk_target(target, n, act);
            walk_expr(value, n, act);
        }
        Update { target, .. } => walk_target(target, n, act),
        Binary { lhs, rhs, .. } | Logical { lhs, rhs, .. } => {
            walk_expr(lhs, n, act);
            walk_expr(rhs, n, act);
        }
        Unary { operand, .. } => walk_expr(operand, n, act),
        Call { callee, args, .. } => {
            walk_expr(callee, n, act);
            for x in args {
                walk_expr(x, n, act);
            }
        }
        Member { object, .. }
        | MemberMaybe { object, .. }
        | GetFixedField { object, .. }
        | ModuleSlot { object, .. }
        | ObjectRest { object, .. } => walk_expr(object, n, act),
        Index { object, index, .. } => {
            walk_expr(object, n, act);
            walk_expr(index, n, act);
        }
        MethodCall { recv, args, .. } | ExtensionCall { recv, args, .. } => {
            walk_expr(recv, n, act);
            for x in args {
                walk_expr(x, n, act);
            }
        }
        NativeMethodCall { object, args, .. } | IntrinsicCall { object, args, .. } => {
            walk_expr(object, n, act);
            for x in args {
                walk_expr(x, n, act);
            }
        }
        Conditional { test, cons, alt } => {
            walk_expr(test, n, act);
            walk_expr(cons, n, act);
            walk_expr(alt, n, act);
        }
        Array(els) | Tuple(els) => {
            for el in els {
                if let HirArrayEl::Expr(x) | HirArrayEl::Spread(x) = el {
                    walk_expr(x, n, act);
                }
            }
        }
        Object { properties } | Record { properties } => {
            for p in properties {
                match p {
                    HirObjectProp::Property { key, value } => {
                        if let HirPropKey::Computed(x) = key {
                            walk_expr(x, n, act);
                        }
                        walk_expr(value, n, act);
                    }
                    HirObjectProp::Spread(x) => walk_expr(x, n, act),
                    // An object method is its own frame.
                    HirObjectProp::Method { func, .. } => walk_stmts(&mut func.body, true, act),
                }
            }
        }
        OptionalChain { object, property } => {
            walk_expr(object, n, act);
            match property {
                HirOptionalProperty::Member(_)
                | HirOptionalProperty::ModuleSlot(_)
                | HirOptionalProperty::Extension(_) => {}
                HirOptionalProperty::Index(x) => walk_expr(x, n, act),
                HirOptionalProperty::Call(args)
                | HirOptionalProperty::MethodCall(_, args)
                | HirOptionalProperty::ExtensionCall(_, args) => {
                    for x in args {
                        walk_expr(x, n, act);
                    }
                }
            }
        }
        Match { subject, cases } => {
            walk_expr(subject, n, act);
            for c in cases {
                if let Some(g) = &mut c.guard {
                    walk_expr(g, n, act);
                }
                walk_stmts(&mut c.body, n, act);
                if let Some(r) = &mut c.result {
                    walk_expr(r, n, act);
                }
            }
        }
        // Everything below runs in a frame that is NOT the module's, so any
        // global it names blocks promotion. This is the whole point of the
        // `in_nested` flag.
        Closure { func, .. } => walk_stmts(&mut func.body, true, act),
        Class(c) => {
            // EVERY body the class carries, not just `methods`. Missing the
            // getters here let a getter that writes a global promote it, and
            // `tests/60-dce-purity.vn` caught it — the exact silent-wrong-code
            // failure this pass has to avoid. Listed field by field so adding
            // one to `HirClass` shows up as an obvious omission right here.
            for m in std::iter::once(&mut c.ctor)
                .chain(c.methods.iter_mut())
                .chain(c.static_methods.iter_mut())
                .chain(c.static_blocks.iter_mut())
            {
                walk_stmts(&mut m.func.body, true, act);
                for d in &mut m.decorators {
                    walk_expr(d, n, act);
                }
            }
            for a in c.getters.iter_mut().chain(c.setters.iter_mut()) {
                walk_stmts(&mut a.func.body, true, act);
            }
            // These are evaluated when the class is BUILT, in the enclosing
            // frame — so they keep the caller's `n`, not `true`.
            if let Some(s) = &mut c.super_class {
                walk_expr(s, n, act);
            }
            for (_, v) in c.static_fields.iter_mut() {
                if let Some(e) = v {
                    walk_expr(e, n, act);
                }
            }
            for d in &mut c.decorators {
                walk_expr(d, n, act);
            }
        }
        Enum(_) => {}
    }
}
