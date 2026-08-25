//! Evolving element-type inference for `let x = []` / `const x = []`
//! (empty array literal, no type annotation). Task A0.3'.
//!
//! **Placement decision.** This hooks directly into the binder's own
//! traversal (`bind_expr`/`bind_stmt`) instead of running as a second,
//! deferred pass (the way `checker_enrichment::enrich_call_returns`
//! enriches `PendingEnrich::Var` after binding finishes). Reason: the
//! live traversal already tracks the exact lexical scope
//! (`Binder::current`) needed to resolve free variables *inside* pushed
//! values — e.g. in `matmul`'s inner loop, `c[row * n + col] = sum`
//! needs `sum`'s type, and `sum` was `let`-bound earlier in the very
//! same block. A deferred AST-only pass would have to rebuild that scope
//! chain from scratch (mirroring every scope-creating construct in
//! lockstep) just to answer a question the live pass answers for free by
//! calling `infer_expr_type(value, Some(self))` at the moment it visits
//! the write. The trade-off: this couples element-type inference to
//! several `bind_expr`/`bind_stmt` match arms rather than isolating it
//! in one function — mitigated by keeping all the actual state and
//! logic in this file and having the call sites be one-line hooks.
//!
//! **Rules** (binding design resolution, see the task brief):
//! 1. Only unannotated `let`/`const x = []` qualify (checked by the
//!    caller in `bind_variable`). Module top-level bindings qualify as
//!    well, and their candidates are finalized by the explicit
//!    `finalize_array_watch(global)` at the end of `Binder::bind` — the
//!    global scope has no block exit to hang it on. What a single-file
//!    scan genuinely cannot account for is a binding that LEAVES the
//!    file, so `bind_export` escapes every exported local: the
//!    declaration form (`export let a = []`), the specifier form
//!    (`export { a }`, whose names never appear as bound identifier
//!    expressions), and `export default <expr>` (whose expression the
//!    binder does not visit at all, so it escapes every open candidate).
//!    Everything else at top level is covered by the same rules as a
//!    function body: a non-exported global is visible only to code in
//!    this file, which this pass has fully scanned by the time the
//!    finalize runs.
//! 2. Whole-scope scan before fixing: every `x.push(e)` / `x[i] = e`
//!    found while the declaring block is being bound (including nested
//!    control-flow blocks — if/while/for/switch/try — but not
//!    closures) unifies into a running element type. Int->Int,
//!    Float->Float; anything else — including Int+Float mixing, which
//!    Varn treats as genuinely distinct types, not a common supertype —
//!    permanently marks the candidate as a conflict, which resolves to
//!    Dynamic (never guess).
//! 3. Escape == Dynamic. Any occurrence of the name that isn't exactly
//!    `x.push(e)`, `x[i]` (read), `x[i] = e` (write), or `x.length`
//!    marks it escaped. Anonymous closures get a **blanket** rule instead
//!    of a precise free-variable scan: entering any closure boundary —
//!    an arrow function body, a function expression, a class/object
//!    method, or a getter/setter (all routed through
//!    `bind_inline_function`/`bind_inline_function_expr`) — escapes
//!    *every* currently open candidate via
//!    `escape_all_open_array_candidates`, whether or not the closure
//!    actually mentions it. A free-variable scan would recover more
//!    cases, but a false negative there — failing to notice a closure
//!    captures and later pushes an incompatible value — would be a
//!    soundness bug (the CLIF backend trusts the checker's proof and
//!    skips guards), not merely a missed optimization. The blanket rule
//!    can't have that failure mode, so it's the one implemented here.
//!
//!    Named function declarations (`function f() {...}`, bound via
//!    `bind_function`) get **no** blanket escape — entering one isn't a
//!    tracked boundary event at all. Soundness there instead falls out
//!    of the write/escape recording being name-based rather than
//!    scope-based: `record_array_write`/`escape_array_candidate` match
//!    the innermost open candidate by name alone, so a write or
//!    escaping use reached from inside a nested named function's body —
//!    visited during the very same linear bind pass — is folded into
//!    the outer candidate's verdict exactly as if it had appeared
//!    inline, before `finalize_array_watch` ever runs on the owning
//!    scope. (Verified sound; do not "fix" by adding a blanket escape
//!    to `bind_function` — see checker_annotations.rs's `Decl::Function`
//!    handling for the annotation-layer side of this.)
//! 4. Zero new type errors — enforced *structurally* by decoupling
//!    optimization-typing from diagnostic-typing. A proved element type is
//!    recorded into `BindResult::evolved_array_types` (an offset-keyed,
//!    optimization-only map consumed solely by `collect_type_annotations`),
//!    and is **never** written onto the symbol's `ty` nor into the
//!    checker's `symbol_types` / `resolved_expr_types`. So for every
//!    diagnostic the checker still sees `x` (and therefore `x[i]`) as
//!    `Dynamic` exactly as it did before this feature existed — the
//!    inference cannot turn a previously-valid program into a type error,
//!    regardless of what it proves. The evolved type surfaces only as
//!    codegen annotations (typed opcodes / CgTy), where being wrong would
//!    at worst be a missed optimization, not a miscompile (the only element
//!    types produced are Int or Float, fixed after the whole block is
//!    scanned and only when nothing disqualified the candidate).

use std::rc::Rc;
use varn_core::TypeKind;

use crate::scope::ScopeId;
use crate::symbol::SymbolId;
use crate::types::Type;

use super::Binder;

/// A `let`/`const x = []` declarator awaiting a verdict. Lives only
/// while its declaring block scope is being bound; removed and
/// finalized by `finalize_array_watch` when that scope exits.
pub(crate) struct ArrayCandidate {
    sym_id: SymbolId,
    name: Rc<str>,
    owner_scope: ScopeId,
    elem_ty: Option<Type>,
    conflict: bool,
    escaped: bool,
}

impl<'r> Binder<'r> {
    /// Registers a qualifying empty-array declarator. Called from
    /// `bind_variable` immediately after the symbol is bound, with
    /// `self.current` still the declaring block's scope.
    pub(crate) fn register_array_candidate(&mut self, sym_id: SymbolId, name: Rc<str>) {
        self.array_watch.push(ArrayCandidate {
            sym_id,
            name,
            owner_scope: self.current,
            elem_ty: None,
            conflict: false,
            escaped: false,
        });
    }

    /// Whether `name` currently refers to an open candidate. Searches
    /// innermost-first so a shadowing re-declaration of the same name in
    /// a nested scope naturally takes priority over an outer candidate
    /// still open further out.
    pub(crate) fn array_candidate_active(&self, name: &str) -> bool {
        !self.array_watch.is_empty() && self.find_candidate(name).is_some()
    }

    fn find_candidate(&self, name: &str) -> Option<&ArrayCandidate> {
        self.array_watch
            .iter()
            .rev()
            .find(|c| c.name.as_ref() == name)
    }

    fn find_candidate_mut(&mut self, name: &str) -> Option<&mut ArrayCandidate> {
        self.array_watch
            .iter_mut()
            .rev()
            .find(|c| c.name.as_ref() == name)
    }

    /// Records a `x.push(value)` / `x[i] = value` write against the
    /// innermost open candidate named `name` (a no-op if none is open).
    /// Rule 2's unification: Int/Float only, anything else is a
    /// conflict.
    pub(crate) fn record_array_write(&mut self, name: &str, value_ty: &Type) {
        if self.array_watch.is_empty() {
            return;
        }
        let normalized = match &value_ty.0 {
            TypeKind::Intrinsic(varn_core::TypeTag::Int) => Some(Type::Int),
            TypeKind::Intrinsic(varn_core::TypeTag::Float) => Some(Type::Float),
            _ => None,
        };
        if let Some(c) = self.find_candidate_mut(name) {
            if c.escaped || c.conflict {
                return;
            }
            match normalized {
                None => c.conflict = true,
                Some(t) => match &c.elem_ty {
                    None => c.elem_ty = Some(t),
                    Some(existing) if *existing == t => {}
                    Some(_) => c.conflict = true,
                },
            }
        }
    }

    /// Marks the innermost open candidate named `name` as escaped (a
    /// no-op if none is open). Rule 3.
    pub(crate) fn escape_array_candidate(&mut self, name: &str) {
        if self.array_watch.is_empty() {
            return;
        }
        if let Some(c) = self.find_candidate_mut(name) {
            c.escaped = true;
        }
    }

    /// Marks every currently open candidate as escaped. Called at every
    /// closure boundary — see the module doc (rule 3) for why this is a
    /// blanket rule rather than a free-variable scan of the closure
    /// body.
    pub(crate) fn escape_all_open_array_candidates(&mut self) {
        for c in self.array_watch.iter_mut() {
            c.escaped = true;
        }
    }

    /// Called whenever a scope that could own a directly-declared
    /// candidate exits (in practice: `StmtKind::Block`; a few other
    /// scope-restoring sites call this too, defensively, for shapes like
    /// an unbraced loop body). Finalizes every candidate whose
    /// `owner_scope` is exactly `scope`.
    ///
    /// A proved element type is recorded into `evolved_array_types` (keyed
    /// by the declarator identifier's source offset), NOT written back onto
    /// the symbol's `ty`. This is deliberate and load-bearing for design
    /// rule 4: the symbol's `ty` is read by the checker's diagnostic typing
    /// (`infer_type_impl` reads `bind.arena.get(sid).ty`), so narrowing it
    /// here would make previously-valid programs fail `vn check` (e.g.
    /// `let a = []; a.push(1); return a[0]` from a `: str` function). The
    /// offset-keyed map is instead an optimization-only channel consumed
    /// solely by `collect_type_annotations`. Candidates that escaped,
    /// conflicted, or never got a consistent write are dropped (the symbol
    /// stays Dynamic, exactly as before this feature existed).
    pub(crate) fn finalize_array_watch(&mut self, scope: ScopeId) {
        if self.array_watch.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.array_watch.len() {
            if self.array_watch[i].owner_scope == scope {
                let c = self.array_watch.remove(i);
                if !c.escaped && !c.conflict {
                    if let Some(elem) = c.elem_ty {
                        let offset = self.arena.get(c.sym_id).offset;
                        self.evolved_array_types.insert(offset, Type::array(elem));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
}
