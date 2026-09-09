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

use rustc_hash::FxHashSet;
use std::rc::Rc;

use varn_tir::{Resolution, TirExpr, TirExprKind, TirModule, TirStmt};

use crate::hir::ctor_summary::{CtorSummaries, SlotInit};
use crate::ssa::ir::VarId;

/// Parent locals / params captured by a nested closure. They must be pinned to
/// a fixed frame slot and read / written through `LoadCaptured` /
/// `StoreCaptured` so the linear-scan allocator never reuses their register
/// while an open upvalue still points at it.
pub(super) fn captured_vars(func: &varn_tir::TirFunction) -> FxHashSet<VarId> {
    use crate::hir::LocalId;
    use varn_tir::TirUpvalue;
    let mut out = FxHashSet::default();
    fn walk_expr(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Closure { upvalues, .. } = &e.kind {
            for u in upvalues {
                match u {
                    TirUpvalue::ParentLocal(i) => {
                        out.insert(VarId::Local(LocalId(*i)));
                    }
                    TirUpvalue::ParentParam(i) => {
                        out.insert(VarId::Param(*i));
                    }
                    TirUpvalue::ParentUpvalue(_) => {}
                }
            }
        }
        for c in child_exprs(e) {
            walk_expr(c, out);
        }
    }
    fn walk_body(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            match stmt {
                TirStmt::Expr(e) | TirStmt::Throw(e) => walk_expr(e, out),
                TirStmt::Let { init: Some(e), .. } | TirStmt::Return(Some(e)) => walk_expr(e, out),
                TirStmt::If { cond, then_body, else_body } => {
                    walk_expr(cond, out);
                    walk_body(then_body, out);
                    walk_body(else_body, out);
                }
                TirStmt::Loop { cond, body } => {
                    walk_expr(cond, out);
                    walk_body(body, out);
                }
                TirStmt::Try { body, catch_body, .. } => {
                    walk_body(body, out);
                    walk_body(catch_body, out);
                }
                _ => {}
            }
        }
    }
    walk_body(&func.body, &mut out);
    out
}

/// Locals / params assigned anywhere inside a `try` region (its guarded body,
/// a `catch` body, or a spliced `finally` copy that landed in either). SSA
/// construction only threads the try-entry's values into a landing pad, so a
/// value mutated on a path that reaches the pad by exception unwinding is lost
/// unless it lives in a fixed frame slot. Mirrors HIR's `scan_pinned_vars`.
pub(super) fn try_pinned_vars(func: &varn_tir::TirFunction) -> FxHashSet<VarId> {
    let mut out = FxHashSet::default();
    fn note_target(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Var = &e.kind {
            match &e.res {
                Resolution::Local(id) => {
                    out.insert(VarId::Local(crate::hir::LocalId(id.0)));
                }
                Resolution::Param(i) => {
                    out.insert(VarId::Param(*i));
                }
                _ => {}
            }
        }
    }
    fn scan_assigns(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            match stmt {
                TirStmt::Expr(e) | TirStmt::Throw(e) | TirStmt::Return(Some(e)) => {
                    scan_expr_assigns(e, out)
                }
                TirStmt::Let { local, .. } => {
                    // a `let` inside the region also needs a stable slot if a
                    // later exception path reads it
                    out.insert(VarId::Local(crate::hir::LocalId(local.0)));
                }
                TirStmt::If { then_body, else_body, .. } => {
                    scan_assigns(then_body, out);
                    scan_assigns(else_body, out);
                }
                TirStmt::Loop { body, .. } => scan_assigns(body, out),
                TirStmt::Try { body, catch_body, .. } => {
                    scan_assigns(body, out);
                    scan_assigns(catch_body, out);
                }
                _ => {}
            }
        }
    }
    fn scan_expr_assigns(e: &TirExpr, out: &mut FxHashSet<VarId>) {
        if let TirExprKind::Assign { target, value } = &e.kind {
            note_target(target, out);
            scan_expr_assigns(value, out);
        }
        for c in child_exprs(e) {
            scan_expr_assigns(c, out);
        }
    }
    fn walk(body: &[TirStmt], out: &mut FxHashSet<VarId>) {
        for stmt in body {
            match stmt {
                TirStmt::If { then_body, else_body, .. } => {
                    walk(then_body, out);
                    walk(else_body, out);
                }
                TirStmt::Loop { body, .. } => walk(body, out),
                TirStmt::Try { body, catch_body, .. } => {
                    scan_assigns(body, out);
                    scan_assigns(catch_body, out);
                    // nested trys inside
                    walk(body, out);
                    walk(catch_body, out);
                }
                _ => {}
            }
        }
    }
    walk(&func.body, &mut out);
    out
}

pub fn collect(tir: &TirModule) -> CtorSummaries {
    let mut reassigned: FxHashSet<Rc<str>> = FxHashSet::default();
    let mut note_reassign = |name: &Rc<str>| {
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
        let ctor_name: Rc<str> = Rc::from(format!("{}.constructor", ci.name));
        let Some(ctor) = tir.functions.iter().find(|f| f.name == ctor_name) else {
            continue;
        };
        if ctor.is_async || ctor.is_generator {
            continue;
        }
        if let Some(slots) = summarize(ctor, ci.fields.len()) {
            out.insert(ci.name.clone(), slots);
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
        if !matches!(object.kind, TirExprKind::Var)
            || !matches!(object.res, Resolution::None)
        {
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

fn scan_body(body: &[TirStmt], tir: &TirModule, note: &mut impl FnMut(&Rc<str>)) {
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
            TirStmt::If { cond, then_body, else_body } => {
                scan_expr(cond, tir, note);
                scan_body(then_body, tir, note);
                scan_body(else_body, tir, note);
            }
            TirStmt::Loop { cond, body } => {
                scan_expr(cond, tir, note);
                scan_body(body, tir, note);
            }
            TirStmt::Try { body, catch_body, .. } => {
                scan_body(body, tir, note);
                scan_body(catch_body, tir, note);
            }
        }
    }
}

fn scan_expr(e: &TirExpr, tir: &TirModule, note: &mut impl FnMut(&Rc<str>)) {
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
    for child in child_exprs(e) {
        scan_expr(child, tir, note);
    }
}

fn arg_exprs(args: &[varn_tir::TirArg]) -> impl Iterator<Item = &TirExpr> {
    args.iter().map(|a| match a {
        varn_tir::TirArg::Expr(x)
        | varn_tir::TirArg::Named { value: x, .. }
        | varn_tir::TirArg::Spread(x) => x,
    })
}

fn child_exprs(e: &TirExpr) -> Vec<&TirExpr> {
    use TirExprKind::*;
    let mut out: Vec<&TirExpr> = Vec::new();
    match &e.kind {
        IntLit(_) | FloatLit(_) | BoolLit(_) | StrLit(_) | CharLit(_) | NullLit | Var
        | Closure { .. } | DecimalLit(_) | BigIntLit(_) => {}
        RangeLit { start, end, .. } => {
            out.push(start);
            out.push(end);
        }
        ObjectRest { object, .. } => out.push(object),
        ExtensionCall { recv, args, .. } => {
            out.push(recv);
            out.extend(arg_exprs(args));
        }
        Binary { lhs, rhs, .. } => {
            out.push(lhs);
            out.push(rhs);
        }
        Unary { operand, .. } | Cast { operand } | ObjectKeys { operand } => out.push(operand),
        Field { object, .. } => out.push(object),
        Index { object, index } => {
            out.push(object);
            out.push(index);
        }
        Call { callee, args } => {
            out.push(callee);
            out.extend(arg_exprs(args));
        }
        MethodCall { recv, args, .. } => {
            out.push(recv);
            out.extend(arg_exprs(args));
        }
        Assign { target, value } => {
            out.push(target);
            out.push(value);
        }
        ArrayLit(els) => {
            for el in els {
                match el {
                    varn_tir::TirArrayEl::Expr(x) | varn_tir::TirArrayEl::Spread(x) => {
                        out.push(x)
                    }
                    varn_tir::TirArrayEl::Hole => {}
                }
            }
        }
        TupleLit(xs) => out.extend(xs.iter()),
        ObjectLit { entries } => {
            for en in entries {
                match en {
                    varn_tir::TirObjectEntry::Field { value, .. } => out.push(value),
                    varn_tir::TirObjectEntry::Spread(x) => out.push(x),
                }
            }
        }
        Await { future } => out.push(future),
        Yield { value, .. } => out.extend(value.as_deref()),
        Discriminant { value } | VariantPayload { value, .. } | TypeTest { value, .. } => {
            out.push(value)
        }
        New { args, .. } | MakeVariant { args } | SuperCall { args }
        | SuperMethodCall { args, .. } => out.extend(arg_exprs(args)),
        Select { cond, then_val, else_val } => {
            out.push(cond);
            out.push(then_val);
            out.push(else_val);
        }
    }
    out
}
