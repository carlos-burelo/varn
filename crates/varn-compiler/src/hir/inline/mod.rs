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

pub(crate) mod candidates;
pub(crate) mod rewrite;
pub(crate) mod traverse;

pub(crate) use candidates::*;
pub(crate) use rewrite::*;
pub(crate) use traverse::*;

use rustc_hash::FxHashMap;
use std::rc::Rc;

use crate::hir::{HirExpr, HirModule};

pub(crate) type Candidates = FxHashMap<Rc<str>, (usize, HirExpr)>;

pub fn run(module: &mut HirModule) {
    let mutated = collect_mutated_globals(module);

    let qualified =
        |name: &Rc<str>| -> Rc<str> { Rc::from(format!("{}::{}", module.source_file, name)) };

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
        if mutated.contains(&global) || mutated.contains(&f.name) {
            continue;
        }
        let Some(expr) = single_expression_body(&f.body) else {
            continue;
        };
        if body_is_inlinable(&expr) {
            candidates.insert(global, (f.params.len(), expr.clone()));
            candidates.insert(f.name.clone(), (f.params.len(), expr));
        }
    }
    if candidates.is_empty() {
        return;
    }

    let names: Vec<Rc<str>> = candidates.keys().cloned().collect();
    for name in names {
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
