use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::ast::{AstArena, Decl, Program, StmtId, StmtKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind, TirStmt, TirUnOp};

use crate::binder::BindResult;

pub(super) struct NestedTypes {
    slots: FxHashMap<Arc<str>, u32>,

    first_ordinal: u32,

    met: RefCell<Vec<(Arc<str>, StmtId)>>,
}

impl NestedTypes {
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
            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
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

    pub(super) fn met<'a>(&self, arena: &'a AstArena) -> Vec<&'a Decl> {
        self.met
            .borrow()
            .iter()
            .filter_map(|(_, s)| match &arena.stmt(*s).kind {
                StmtKind::Decl(d) => Some(d.as_ref()),
                StmtKind::Block { .. } | StmtKind::Empty | StmtKind::Expr { .. } | StmtKind::Error | StmtKind::If { .. } | StmtKind::While { .. } | StmtKind::DoWhile { .. } | StmtKind::For { .. } | StmtKind::ForIn { .. } | StmtKind::ForOf { .. } | StmtKind::Switch { .. } | StmtKind::Return { .. } | StmtKind::Break { .. } | StmtKind::Continue { .. } | StmtKind::Throw { .. } | StmtKind::Try { .. } | StmtKind::Using { .. } | StmtKind::Labeled { .. } | StmtKind::Debugger => None,
            })
            .collect()
    }
}

pub(super) fn top_level_types(
    program: &Program,
    arena: &AstArena,
    interner: &varn_core::AtomInterner,
) -> FxHashSet<Arc<str>> {
    let type_name = |d: &Decl| {
        super::decl_classify::class_decl(d)
            .and_then(|c| c.id)
            .or_else(|| super::decl_classify::enum_decl(d).map(|e| e.id))
            .or_else(|| super::decl_classify::anon_class_of(d, arena).and_then(|c| c.id))
    };
    let mut out = FxHashSet::default();
    for &stmt in &program.body {
        let StmtKind::Decl(d) = &arena.stmt(stmt).kind else {
            continue;
        };
        let mut decls = vec![d.as_ref()];
        if let Some(ns) = super::decl_classify::namespace_decl(d) {
            decls.extend(super::namespaces::ns_nested_types(ns));
        }
        for id in decls.into_iter().filter_map(type_name) {
            out.insert(Arc::from(interner.resolve(id)));
        }
    }
    out
}
