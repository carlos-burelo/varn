use super::super::Checker;
use super::assigned_targets::{assigned_in_stmt, static_path_atoms, AssignedTargets, MemberRoot};
use crate::checker::recorder::Recorder;
use std::sync::Arc;
use varn_core::ast::{AssignOp, ExprId, ExprKind, StmtId, StmtKind};
use varn_core::Atom;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn forget_symbol(&mut self, rec: &Recorder, id: SymbolId, bind: &BindResult) {
        let declared = rec
            .symbol_types
            .get(&id)
            .copied()
            .or_else(|| bind.arena.get(id).ty)
            .unwrap_or(Type::Dynamic);
        let Some(stack) = self.narrowed_types.get_mut(&id) else {
            return;
        };
        for held in stack.iter_mut() {
            *held = declared;
        }
        self.mark_infer_env_dirty();
    }

    pub(crate) fn forget_narrowing(&mut self, rec: &Recorder, name: Atom, bind: &BindResult) {
        let Some(id) = bind
            .scopes
            .get(self.current_scope)
            .resolve(name, &bind.scopes)
        else {
            return;
        };
        self.forget_symbol(rec, id, bind);
    }

    pub(crate) fn forget_assigned_target(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        bind: &BindResult,
    ) {
        if let ExprKind::Identifier { name } = &self.ast_arena.expr(target).kind {
            self.forget_narrowing(rec, *name, bind);
            return;
        }
        if let Some(key) = self.member_path_key(target, bind)
            && self.member_narrowed.remove(&key).is_some()
        {
            self.mark_infer_env_dirty();
        }
    }

    pub(crate) fn forget_all_members(&mut self) {
        if !self.member_narrowed.is_empty() {
            self.member_narrowed.clear();
            self.mark_infer_env_dirty();
        }
    }

    pub(crate) fn member_path_key(&self, expr: ExprId, bind: &BindResult) -> Option<MemberKey> {
        let (root, path) = static_path_atoms(expr, self.ast_arena)?;
        let root = match root {
            MemberRoot::This(_) => None,
            MemberRoot::Name(_, name) => Some(
                bind.scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)?,
            ),
        };
        let path = path
            .into_iter()
            .map(|atom| Arc::from(bind.interner.resolve(atom)))
            .collect();
        Some((root, path))
    }

    pub(crate) fn narrowed_member_read(&mut self, expr: ExprId, bind: &BindResult) -> Option<Type> {
        let (root, path) = static_path_atoms(expr, self.ast_arena)?;
        let root = match root {
            MemberRoot::This(_) => None,
            MemberRoot::Name(_, name) => Some(
                bind.scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)?,
            ),
        };
        let key: MemberKey = (root, Self::atoms_to_path(&path, bind));
        self.member_narrowed
            .get(&key)
            .and_then(|stack| stack.last())
            .copied()
    }

    pub(crate) fn member_path_declared(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<(MemberKey, Type)> {
        let (root_expr, key) = match static_path_atoms(expr, self.ast_arena) {
            Some((MemberRoot::This(node), path)) => {
                (node, (None, Self::atoms_to_path(&path, bind)))
            }
            Some((MemberRoot::Name(node, name), path)) => {
                let id = bind
                    .scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)?;
                (node, (Some(id), Self::atoms_to_path(&path, bind)))
            }
            None => return None,
        };
        let mut ty = self.infer_type(rec, root_expr, bind);
        for name in &key.1 {
            if ty.is_dynamic() {
                return None;
            }
            let (next, _) = self.find_member_info(&ty, name, bind)?;
            if next.is_dynamic() {
                return None;
            }
            ty = next;
        }
        Some((key, ty))
    }

    pub(super) fn atoms_to_path(path: &[Atom], bind: &BindResult) -> Vec<Arc<str>> {
        path.iter()
            .map(|atom| Arc::from(bind.interner.resolve(*atom)))
            .collect()
    }

    pub(crate) fn forget_loop_assignments(
        &mut self,
        rec: &Recorder,
        stmt: StmtId,
        bind: &BindResult,
    ) {
        let mut targets = AssignedTargets::empty();
        assigned_in_stmt(stmt, self.ast_arena, &mut targets);
        for name in targets.vars {
            self.forget_narrowing(rec, name, bind);
        }
        for (root, path) in targets.members {
            let resolved = match root {
                MemberRoot::This(_) => Some(None),
                MemberRoot::Name(_, name) => bind
                    .scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
                    .map(Some),
            };
            let Some(id) = resolved else {
                continue;
            };
            let key: MemberKey = (id, Self::atoms_to_path(&path, bind));
            if self.member_narrowed.remove(&key).is_some() {
                self.mark_infer_env_dirty();
            }
        }
    }
}

pub(crate) type MemberKey = (Option<SymbolId>, Vec<Arc<str>>);

impl<'r> Checker<'r> {
    pub(crate) fn member_narrowing_after_assignment(
        &mut self,
        rec: &mut Recorder,
        stmt: StmtId,
        bind: &BindResult,
    ) -> Vec<(MemberKey, Type)> {
        let StmtKind::Expr { expression } = &self.ast_arena.stmt(stmt).kind else {
            return Vec::new();
        };
        let ExprKind::Assign { op, target, value } = &self.ast_arena.expr(*expression).kind else {
            return Vec::new();
        };
        let fills = match op {
            AssignOp::Assign | AssignOp::NullishAssign | AssignOp::OrAssign => true,
            AssignOp::AndAssign
            | AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign
            | AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign
            | AssignOp::UShrAssign => false,
        };
        if !fills {
            return Vec::new();
        }
        let (target, value) = (*target, *value);
        let Some((key, declared)) = self.member_path_declared(rec, target, bind) else {
            return Vec::new();
        };
        if declared.is_dynamic() || !declared.is_nullable(&self.ty_table) {
            return Vec::new();
        }
        let given = self.infer_type(rec, value, bind);
        if given.is_dynamic() || given.is_nullable(&self.ty_table) || given == Type::Null {
            return Vec::new();
        }
        let present = declared.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        vec![(key, present)]
    }

    pub(crate) fn narrowing_after_assignment(
        &mut self,
        rec: &mut Recorder,
        stmt: StmtId,
        bind: &BindResult,
    ) -> Vec<(SymbolId, Type)> {
        let StmtKind::Expr { expression } = &self.ast_arena.stmt(stmt).kind else {
            return Vec::new();
        };
        let ExprKind::Assign { op, target, value } = &self.ast_arena.expr(*expression).kind else {
            return Vec::new();
        };
        let fills = match op {
            AssignOp::Assign | AssignOp::NullishAssign | AssignOp::OrAssign => true,
            AssignOp::AndAssign
            | AssignOp::AddAssign
            | AssignOp::SubAssign
            | AssignOp::MulAssign
            | AssignOp::DivAssign
            | AssignOp::ModAssign
            | AssignOp::PowAssign
            | AssignOp::BitAndAssign
            | AssignOp::BitOrAssign
            | AssignOp::BitXorAssign
            | AssignOp::ShlAssign
            | AssignOp::ShrAssign
            | AssignOp::UShrAssign => false,
        };
        let ExprKind::Identifier { name } = &self.ast_arena.expr(*target).kind else {
            return Vec::new();
        };
        if !fills {
            return Vec::new();
        }
        let Some(id) = bind
            .scopes
            .get(self.current_scope)
            .resolve(*name, &bind.scopes)
        else {
            return Vec::new();
        };
        let Some(declared) = rec
            .symbol_types
            .get(&id)
            .copied()
            .or_else(|| bind.arena.get(id).ty)
        else {
            return Vec::new();
        };
        if declared.is_dynamic() || !declared.is_nullable(&self.ty_table) {
            return Vec::new();
        }
        let given = self.infer_type(rec, *value, bind);
        if given.is_dynamic() || given.is_nullable(&self.ty_table) || given == Type::Null {
            return Vec::new();
        }
        let present = declared.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        vec![(id, present)]
    }
}
