//! What `new C(a, b)` actually does to a fresh instance's field slots.
//!
//! Consumed by `passes::escape`, which uses it to answer a `getfixed` on a
//! non-escaping instance from the constructor's argument directly — and then
//! delete the construction, allocation included.
//!
//! # Why this is sound
//!
//! `new C(x)` lowers to a plain `Call` on whatever the global `C` holds, and
//! **class bindings are reassignable** — `Box = Other` is legal and changes
//! what `new Box(..)` builds. Baking "this global is that class" would be a
//! silent miscompile.
//!
//! Two facts make a compile-time proof possible instead of a runtime guard:
//!
//! 1. Globals are qualified by the file that *declares the binding*, and an
//!    importing module gets its OWN cell — `import { Box }` in `main.vn`
//!    produces `main.vn::Box`, and assigning it there leaves `lib.vn::Box`
//!    untouched (verified: the defining module's own `new Box(..)` keeps
//!    building the original class).
//! 2. So a call site naming `<M>::C` inside module `M` can only ever reach
//!    the class `M` declared, provided `M` itself never reassigns it.
//!
//! A class therefore qualifies only when the module assigns its global
//! EXACTLY ONCE — the declaration. The scan below must consequently reach
//! every statement, including nested bodies and functions inside expressions;
//! a missed reassignment is a miscompile, not a missed optimization.

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;

use super::inline::{for_each_stmt_expr, push_child_stmts, walk_exprs};
use super::{HirAssignTarget, HirBinding, HirClass, HirExpr, HirModule, HirStmt};

/// Where one field slot's value comes from once the constructor has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotInit {
    /// Constructor parameter `n` — i.e. the call site's argument `n`.
    Param(u32),
    /// Never written by the constructor, so the slot reads as null.
    Null,
}

/// Qualified class global → per-slot initializer, in declared field order.
pub type CtorSummaries = FxHashMap<Rc<str>, Vec<SlotInit>>;

thread_local! {
    static CURRENT: RefCell<Rc<CtorSummaries>> = RefCell::new(Rc::new(CtorSummaries::default()));
}

/// The summaries in force for the module being lowered.
///
/// Scoped rather than threaded as a parameter because the consumer is
/// `passes::optimize`, which the emitter reaches through
/// `lower::lower_function` while emitting a nested closure or method — a dozen
/// signatures away from the only place that has the `HirModule`. The same
/// arrangement `clif_link::CtxGuard` uses on the VM side, and with the same
/// rule: the guard restores the previous value on drop, so nested lowerings
/// compose.
pub struct Scope(Rc<CtorSummaries>);

impl Scope {
    pub fn enter(summaries: CtorSummaries) -> Self {
        let prev = CURRENT.with(|c| c.replace(Rc::new(summaries)));
        Scope(prev)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| *c.borrow_mut() = Rc::clone(&self.0));
    }
}

/// Summaries for the module currently being lowered; empty outside a
/// [`Scope`], which makes `passes::escape` a no-op rather than a hazard.
pub fn current() -> Rc<CtorSummaries> {
    CURRENT.with(|c| Rc::clone(&c.borrow()))
}

/// Summaries for every class in `module` that is provably stable and whose
/// constructor does nothing but copy parameters into its own fields.
pub fn collect(module: &HirModule) -> CtorSummaries {
    let mut scan = Scan::default();
    scan.function(&module.top_level);
    for f in &module.functions {
        scan.function(f);
    }

    let mut out = CtorSummaries::default();
    for (name, class) in &scan.declared {
        if scan.rebound.contains(name) || scan.declared_twice.contains(name) {
            continue;
        }
        if let Some(slots) = summarize(class) {
            out.insert(name.clone(), slots);
        }
    }
    out
}

/// The constructor's effect, or `None` when it does anything this pass cannot
/// reproduce at the call site.
fn summarize(class: &HirClass) -> Option<Vec<SlotInit>> {
    // A superclass brings inherited fields and a `super(..)` call whose
    // effects are not in this body.
    if class.super_class.is_some() || !class.decorators.is_empty() {
        return None;
    }
    // An accessor can give a field name behaviour a slot read does not have.
    if !class.getters.is_empty() || !class.setters.is_empty() {
        return None;
    }
    let ctor = &class.ctor.func;
    if ctor.is_async || ctor.is_generator || ctor.has_rest {
        return None;
    }
    // A defaulted parameter means the call site's argument list and the
    // parameter list are not the same thing.
    if ctor.params.iter().any(|p| p.default.is_some()) {
        return None;
    }
    // Upvalues would make the body depend on state the call site cannot see.
    if ctor.upvalue_count > 0 || !class.ctor.upvalues.is_empty() {
        return None;
    }

    let mut slots = vec![SlotInit::Null; class.fields.len()];
    let mut written = vec![false; class.fields.len()];
    for stmt in &ctor.body {
        // Straight-line `this.<field> = <param>` only. Anything else —
        // control flow, a call, a store to another object, a read of `this` —
        // and the whole constructor is out.
        let HirStmt::SetFixedField {
            object,
            slot,
            value,
        } = stmt
        else {
            return None;
        };
        if !matches!(object, HirExpr::This) {
            return None;
        }
        let HirExpr::Var(HirBinding::Param(p)) = value else {
            return None;
        };
        let idx = *slot as usize;
        // A slot written twice, or one outside the declared fields, means the
        // model here does not match the object being built.
        if idx >= slots.len() || written[idx] {
            return None;
        }
        written[idx] = true;
        slots[idx] = SlotInit::Param(*p);
    }
    Some(slots)
}

#[derive(Default)]
struct Scan {
    /// Global → the class its single declaring assignment builds.
    declared: FxHashMap<Rc<str>, HirClass>,
    /// Globals declared as a class more than once.
    declared_twice: FxHashSet<Rc<str>>,
    /// Globals assigned by anything other than a class declaration.
    rebound: FxHashSet<Rc<str>>,
}

impl Scan {
    fn function(&mut self, f: &super::HirFunction) {
        self.stmts(&f.body);
    }

    fn stmts(&mut self, stmts: &[HirStmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &HirStmt) {
        if let HirStmt::Assign {
            target: HirBinding::Global(g),
            value,
        } = s
        {
            match value {
                HirExpr::Class(c) => {
                    if self.declared.insert(g.clone(), (**c).clone()).is_some() {
                        self.declared_twice.insert(g.clone());
                    }
                }
                _ => {
                    self.rebound.insert(g.clone());
                }
            }
        }

        // Expressions of this statement: assignment/update expressions that
        // target a global, plus the function bodies `walk_exprs` refuses to
        // descend into. Collected first because the closure cannot also hold
        // `&mut self`.
        let mut rebound: Vec<Rc<str>> = Vec::new();
        let mut nested: Vec<&HirExpr> = Vec::new();
        for_each_stmt_expr(s, &mut |e| {
            walk_exprs(e, &mut |inner| match inner {
                HirExpr::Assign { target: t, .. } | HirExpr::Update { target: t, .. } => {
                    if let HirAssignTarget::Var(HirBinding::Global(g)) = &**t {
                        rebound.push(g.clone());
                    }
                }
                // `walk_exprs` stops at these four; their bodies can reassign
                // globals just as well.
                HirExpr::Closure { .. }
                | HirExpr::Class(_)
                | HirExpr::Enum(_)
                | HirExpr::Match { .. } => nested.push(inner),
                _ => {}
            });
        });
        for g in rebound {
            self.rebound.insert(g);
        }
        for e in nested {
            self.nested_expr(e);
        }

        // Nested statement bodies (if / loops / switch / try). Missing one
        // would hide a reassignment, which is the one error this scan may not
        // make.
        let mut kids: Vec<&HirStmt> = Vec::new();
        push_child_stmts(s, &mut kids);
        for k in kids {
            self.stmt(k);
        }
    }

    fn nested_expr(&mut self, e: &HirExpr) {
        match e {
            HirExpr::Closure { func, .. } => self.function(func),
            HirExpr::Class(c) => {
                self.function(&c.ctor.func);
                for m in c
                    .methods
                    .iter()
                    .chain(&c.static_methods)
                    .chain(&c.static_blocks)
                {
                    self.function(&m.func);
                }
                for a in c.getters.iter().chain(&c.setters) {
                    self.function(&a.func);
                }
            }
            HirExpr::Match { cases, .. } => {
                for c in cases {
                    self.stmts(&c.body);
                }
            }
            _ => {}
        }
    }
}
