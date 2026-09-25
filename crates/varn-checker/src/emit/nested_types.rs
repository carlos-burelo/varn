//! Class and enum declarations nested in a function body (or a block).
//!
//! The checker's type tables are keyed by name at module scope: a class or
//! enum declared inside a function is the same kind of nominal type as one
//! declared at the top level, with one `ClassId` / `EnumId`, one runtime class
//! object held in a module global, and every use loading that global. What
//! differs is only when it comes into existence: a top-level declaration
//! builds its class where it stands in the module body, a nested one where it
//! stands in its function — the first time that declaration runs. So each
//! nested type gets its global slot up front, and its declaration lowers to
//! "build it unless that global already holds it"; its class definition takes
//! the next ordinal after the top-level ones, in the order the declarations
//! are met.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::ast::{AstArena, Decl, Program, StmtId, StmtKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind, TirStmt, TirUnOp};

use crate::binder::BindResult;

/// The nested types of a module, and the declarations met so far.
pub(super) struct NestedTypes {
    /// Each nested type's module global.
    slots: FxHashMap<Arc<str>, u32>,
    /// The `class_defs` ordinal the first nested type takes.
    first_ordinal: u32,
    /// Declarations met, by the ordinal they took.
    met: RefCell<Vec<(Arc<str>, StmtId)>>,
}

impl NestedTypes {
    /// Every class and enum the module declares that is not at its top level
    /// or in a namespace (`top_level`). Each is given a global slot after
    /// `next_slot`; the new slots are returned in order for the caller to
    /// register.
    pub(super) fn collect(
        bind: &BindResult,
        top_level: &FxHashSet<Arc<str>>,
        taken: &FxHashMap<Arc<str>, u32>,
        next_slot: u32,
        first_ordinal: u32,
    ) -> Self {
        let mut names: Vec<Arc<str>> = bind
            .type_members
            .classes
            .keys()
            .chain(bind.type_members.enums.keys())
            .chain(bind.sum_type_variants.keys())
            .filter(|n| !top_level.contains(n.as_ref()))
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        let mut slots = FxHashMap::default();
        let mut next = next_slot;
        for name in names {
            let slot = match taken.get(&name) {
                Some(&s) => s,
                None => {
                    next += 1;
                    next - 1
                }
            };
            slots.insert(name, slot);
        }
        NestedTypes {
            slots,
            first_ordinal,
            met: RefCell::new(Vec::new()),
        }
    }

    /// The new globals, `(name, slot)`, in slot order.
    pub(super) fn new_globals(&self, taken: &FxHashMap<Arc<str>, u32>) -> Vec<(Arc<str>, u32)> {
        let mut out: Vec<(Arc<str>, u32)> = self
            .slots
            .iter()
            .filter(|(n, _)| !taken.contains_key(n.as_ref()))
            .map(|(n, &s)| (n.clone(), s))
            .collect();
        out.sort_by_key(|(_, s)| *s);
        out
    }

    /// The statement a nested declaration `stmt` of type `name` lowers to:
    /// build the class once, where the declaration runs.
    pub(super) fn declare(&self, name: &str, stmt: StmtId) -> Option<TirStmt> {
        let slot = *self.slots.get(name)?;
        let mut met = self.met.borrow_mut();
        let ordinal = match met.iter().position(|(n, _)| n.as_ref() == name) {
            Some(i) => i,
            None => {
                met.push((Arc::from(name), stmt));
                met.len() - 1
            }
        };
        let global = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::GlobalSlot(slot),
            span: Span::EMPTY,
        };
        let unbuilt = TirExpr {
            kind: TirExprKind::Unary {
                op: TirUnOp::IsNull,
                operand: Box::new(global),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        Some(TirStmt::If {
            cond: unbuilt,
            then_body: vec![TirStmt::BuildClass(self.first_ordinal + ordinal as u32)],
            else_body: vec![],
        })
    }

    /// The declarations met, in ordinal order, to emit their class
    /// definitions after the top-level ones.
    pub(super) fn met<'a>(&self, arena: &'a AstArena) -> Vec<&'a Decl> {
        self.met
            .borrow()
            .iter()
            .filter_map(|(_, s)| match &arena.stmt(*s).kind {
                StmtKind::Decl(d) => Some(d.as_ref()),
                _ => None,
            })
            .collect()
    }
}

/// The class and enum names declared at the module's top level, directly or
/// in a namespace.
pub(super) fn top_level_types(
    program: &Program,
    arena: &AstArena,
    interner: &varn_core::AtomInterner,
) -> FxHashSet<Arc<str>> {
    let type_name = |d: &Decl| {
        super::class_decl(d)
            .and_then(|c| c.id)
            .or_else(|| super::enum_decl(d).map(|e| e.id))
            .or_else(|| super::anon_class_of(d, arena).and_then(|c| c.id))
    };
    let mut out = FxHashSet::default();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &arena.stmt(stmt).kind else {
            continue;
        };
        let mut decls = vec![d.as_ref()];
        if let Some(ns) = super::namespace_decl(d) {
            decls.extend(super::ns_nested_types(ns));
        }
        for id in decls.into_iter().filter_map(type_name) {
            out.insert(Arc::from(interner.resolve(id)));
        }
    }
    out
}
