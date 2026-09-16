//! Which `this.<field> = value` assignments a constructor body makes on
//! EVERY path through it.
//!
//! Shallow on purpose, same spirit as `checker::definite_assignment`: straight
//! -line statements and two-armed `if` merge by intersection; loops, `switch`,
//! `try` and anything else contribute nothing (a field only assigned inside
//! one of those is NOT counted as guaranteed — conservative in the safe
//! direction, since the caller only uses this to decide whether a field can
//! be read as `null`). A false negative here just costs a field its static
//! kind (`Dynamic` instead of `Int`/`Float`/`Ref`), never correctness; a false
//! positive would let a genuinely-unassigned field claim a non-nullable type,
//! which is the bug this exists to avoid.
use rustc_hash::FxHashSet;
use std::rc::Rc;
use varn_core::ast::operators::AssignOp;
use varn_core::ast::{Expr, ExprKind, Stmt, StmtKind};

/// Fields `this.<name> = value` assigns on every path through `body`.
pub(super) fn fields_assigned_on_every_path(body: &Stmt) -> FxHashSet<Rc<str>> {
    let mut out = FxHashSet::default();
    walk_stmt(body, &mut out);
    out
}

fn this_field_target(e: &Expr) -> Option<Rc<str>> {
    match &e.kind {
        ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } => {
            if !matches!(object.kind, ExprKind::This) {
                return None;
            }
            match &property.kind {
                ExprKind::Identifier { name } => Some(name.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn walk_expr(e: &Expr, out: &mut FxHashSet<Rc<str>>) {
    if let ExprKind::Assign {
        op: AssignOp::Assign,
        target,
        ..
    } = &e.kind
    {
        if let Some(name) = this_field_target(target) {
            out.insert(name);
        }
    }
}

/// Statements executed unconditionally in sequence: assignments accumulate
/// (union), since reaching any one of them on the straight-line path is
/// enough. `If` merges its two branches by INTERSECTION — only a field both
/// sides assign is guaranteed no matter which one ran.
fn walk_stmt(s: &Stmt, out: &mut FxHashSet<Rc<str>>) {
    match &s.kind {
        StmtKind::Block { stmts } => {
            for inner in stmts {
                walk_stmt(inner, out);
            }
        }
        StmtKind::Expr { expression } => walk_expr(expression, out),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            let mut then_set = FxHashSet::default();
            walk_stmt(consequent, &mut then_set);
            let else_set = match alternate {
                Some(alt) => {
                    let mut s = FxHashSet::default();
                    walk_stmt(alt, &mut s);
                    s
                }
                // No `else`: the branch that skips `consequent` assigns
                // nothing new, so nothing from `then_set` can be guaranteed.
                None => FxHashSet::default(),
            };
            out.extend(then_set.intersection(&else_set).cloned());
        }
        // Loops, `switch`, `try`/`catch`, and everything else: a field
        // assigned only inside one of these is not guaranteed (the body may
        // run zero times, or take an untraced path) — contribute nothing.
        _ => {}
    }
}
