//! `hir::ctor_summary` ported to TIR.
//!
//! Same product — a per-class map from field slot to "the value that slot
//! holds once `new C(..)` returns" — feeding the same consumer,
//! `passes::escape`. The soundness argument is simpler here than on HIR: the
//! checker resolved `new C(..)` to a `ClassId` directly, so there is no
//! "is this global still the class it was declared as" question. The one
//! remaining hazard is source that reassigns the class's own global
//! (`C = something`), which `from_tir` lowers to a `Call` on `LoadGlobal("C")`
//! — the exact shape `escape` keys on — so a class whose name is assigned
//! anywhere is dropped.

#![allow(dead_code)]

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use varn_tir::{Resolution, TirExpr, TirExprKind, TirModule, TirStmt};

use super::tir_children::child_exprs;

/// Where one field slot's value comes from once the constructor has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotInit {
    /// Constructor parameter `n` — i.e. the call site's argument `n`.
    Param(u32),
    /// Never written by the constructor, so the slot reads as null.
    Null,
}

/// Per-class field initializers, in declared field order, reachable by the two
/// ways SSA names a class global: by name (`LoadGlobal`) and by the module's
/// numbered global slot (`LoadGlobalIdx`).
#[derive(Default)]
pub struct CtorSummaries {
    by_name: FxHashMap<Arc<str>, Vec<SlotInit>>,
    by_slot: FxHashMap<u32, Vec<SlotInit>>,
}

impl CtorSummaries {
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.by_slot.is_empty()
    }

    pub fn by_name(&self, name: &str) -> Option<&Vec<SlotInit>> {
        self.by_name.get(name)
    }

    pub fn by_slot(&self, slot: u32) -> Option<&Vec<SlotInit>> {
        self.by_slot.get(&slot)
    }
}

thread_local! {
    static CURRENT: RefCell<Rc<CtorSummaries>> = RefCell::new(Rc::new(CtorSummaries::default()));
}

/// Scoped summaries guard in force for the module being lowered.
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

/// Current summaries for the module being compiled; empty outside a `Scope`.
pub fn current() -> Rc<CtorSummaries> {
    CURRENT.with(|c| Rc::clone(&c.borrow()))
}

pub fn collect(tir: &TirModule) -> CtorSummaries {
    let mut reassigned: FxHashSet<Arc<str>> = FxHashSet::default();
    let mut note_reassign = |name: &Arc<str>| {
        reassigned.insert(name.clone());
    };
    for f in std::iter::once(&tir.top_level).chain(&tir.functions) {
        scan_body(&f.body, tir, &mut note_reassign);
    }

    let mut out = CtorSummaries::default();
    for ci in &tir.classes {
        if reassigned.contains(&ci.name) || ci.parent.is_some() {
            continue;
        }
        let ctor_name: Arc<str> = Arc::from(format!("{}.constructor", ci.name));
        let Some(ctor) = tir.functions.iter().find(|f| f.name == ctor_name) else {
            continue;
        };
        if ctor.is_async || ctor.is_generator {
            continue;
        }
        if let Some(slots) = summarize(ctor, ci.fields.len()) {
            let qualified: Arc<str> = Arc::from(format!(
                "{}::{}",
                tir.source_file.replace('\\', "/"),
                ci.name
            ));
            if let Some(slot) = tir.global_names.iter().position(|n| *n == ci.name) {
                out.by_slot.insert(slot as u32, slots.clone());
            }
            out.by_name.insert(qualified, slots.clone());
            out.by_name.insert(ci.name.clone(), slots);
        }
    }
    out
}

/// The constructor's effect, or `None` when it does anything the call site
/// cannot reproduce: only straight-line `this.<field> = <param>` is allowed.
fn summarize(ctor: &varn_tir::TirFunction, field_count: usize) -> Option<Vec<SlotInit>> {
    let mut slots = vec![SlotInit::Null; field_count];
    let mut written = vec![false; field_count];
    for stmt in &ctor.body {
        let TirStmt::Expr(e) = stmt else { return None };
        let TirExprKind::Assign { target, value } = &e.kind else {
            return None;
        };
        let TirExprKind::Field { object, .. } = &target.kind else {
            return None;
        };
        // `this.<field>` — a `Var` node with no resolution is `this`.
        if !matches!(object.kind, TirExprKind::Var) || !matches!(object.res, Resolution::None) {
            return None;
        }
        let Resolution::FieldSlot(slot) = target.res else {
            return None;
        };
        if !matches!(value.kind, TirExprKind::Var) {
            return None;
        }
        let Resolution::Param(p) = value.res else {
            return None;
        };
        let idx = slot as usize;
        if idx >= slots.len() || written[idx] {
            return None;
        }
        written[idx] = true;
        slots[idx] = SlotInit::Param(p);
    }
    Some(slots)
}

fn scan_body(body: &[TirStmt], tir: &TirModule, note: &mut impl FnMut(&Arc<str>)) {
    for stmt in body {
        match stmt {
            TirStmt::Expr(e) | TirStmt::Throw(e) => scan_expr(e, tir, note),
            TirStmt::Let { init: Some(e), .. } | TirStmt::Return(Some(e)) => {
                scan_expr(e, tir, note)
            }
            TirStmt::Let { .. }
            | TirStmt::Return(None)
            | TirStmt::Break
            | TirStmt::Continue
            | TirStmt::BuildClass(_) => {}
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                scan_expr(cond, tir, note);
                scan_body(then_body, tir, note);
                scan_body(else_body, tir, note);
            }
            TirStmt::Loop { cond, body } => {
                scan_expr(cond, tir, note);
                scan_body(body, tir, note);
            }
            TirStmt::Try {
                body, catch_body, ..
            } => {
                scan_body(body, tir, note);
                scan_body(catch_body, tir, note);
            }
        }
    }
}

fn scan_expr(e: &TirExpr, tir: &TirModule, note: &mut impl FnMut(&Arc<str>)) {
    if let TirExprKind::Assign { target, .. } = &e.kind {
        if matches!(target.kind, TirExprKind::Var) {
            match &target.res {
                Resolution::GlobalSlot(n) => {
                    if let Some(name) = tir.global_names.get(*n as usize) {
                        note(name);
                    }
                }
                Resolution::ByName { name, .. } => note(name),
                _ => {}
            }
        }
    }
    if let TirExprKind::Seq { stmts, .. } = &e.kind {
        scan_body(stmts, tir, note);
    }
    for child in child_exprs(e) {
        scan_expr(child, tir, note);
    }
}
