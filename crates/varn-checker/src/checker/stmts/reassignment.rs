use super::super::Checker;
use crate::checker::definite_assignment::walk_expr_children;
use crate::checker::recorder::Recorder;
use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::{ArrowBody, AssignOp, AstArena, Decl, ExprId, ExprKind, ForInit, Pattern, StmtId, StmtKind};
use varn_core::Atom;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn forget_symbol(
        &mut self,
        rec: &Recorder,
        id: SymbolId,
        bind: &BindResult,
    ) {
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
        if let Some(key) = self.member_path_key(target, bind) {
            if self.member_narrowed.remove(&key).is_some() {
                self.mark_infer_env_dirty();
            }
        }
    }

    pub(crate) fn forget_all_members(&mut self) {
        if !self.member_narrowed.is_empty() {
            self.member_narrowed.clear();
            self.mark_infer_env_dirty();
        }
    }

    pub(crate) fn member_path_key(
        &self,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<MemberKey> {
        let (root, path) = static_path_atoms(expr, self.ast_arena)?;
        let root = match root {
            MemberRoot::This(_) => None,
            MemberRoot::Name(_, name) => {
                Some(bind.scopes.get(self.current_scope).resolve(name, &bind.scopes)?)
            }
        };
        let path = path
            .into_iter()
            .map(|atom| Arc::from(bind.interner.resolve(atom)))
            .collect();
        Some((root, path))
    }

    pub(crate) fn narrowed_member_read(
        &mut self,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<Type> {
        let (root, path) = static_path_atoms(expr, self.ast_arena)?;
        let root = match root {
            MemberRoot::This(_) => None,
            MemberRoot::Name(_, name) => {
                Some(bind.scopes.get(self.current_scope).resolve(name, &bind.scopes)?)
            }
        };
        let key: MemberKey = (root, Self::atoms_to_path(&path, bind));
        self.member_narrowed.get(&key).and_then(|stack| stack.last()).copied()
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

    fn atoms_to_path(path: &[Atom], bind: &BindResult) -> Vec<Arc<str>> {
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

pub(crate) enum ClosureBodyRef {
    Stmt(StmtId),
    Expr(ExprId),
}

pub(crate) type MemberKey = (Option<SymbolId>, Vec<Arc<str>>);

#[derive(Clone, Default)]
pub(crate) struct ClosureEffects {
    pub vars: Vec<SymbolId>,
    pub members: Vec<MemberKey>,
}

impl<'r> Checker<'r> {
    pub(crate) fn closure_assigned_symbols(
        &mut self,
        body: ClosureBodyRef,
        bind: &BindResult,
    ) -> ClosureEffects {
        let mut targets = AssignedTargets::empty();
        match body {
            ClosureBodyRef::Stmt(stmt) => assigned_in_stmt(stmt, self.ast_arena, &mut targets),
            ClosureBodyRef::Expr(expr) => assigned_in_expr(expr, self.ast_arena, &mut targets),
        }
        let mut ids: Vec<SymbolId> = targets
            .vars
            .into_iter()
            .filter_map(|name| {
                bind.scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
            })
            .collect();
        ids.sort();
        ids.dedup();
        let mut members = Vec::new();
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
            members.push((id, Self::atoms_to_path(&path, bind)));
        }
        members.sort();
        members.dedup();
        ClosureEffects {
            vars: ids,
            members,
        }
    }

    pub(crate) fn closure_def_effects(
        &mut self,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<ClosureEffects> {
        let body = match &self.ast_arena.expr(expr).kind {
            ExprKind::Function { body, .. } => ClosureBodyRef::Stmt(*body),
            ExprKind::Arrow { body, .. } => match body.as_ref() {
                ArrowBody::Block(stmt) => ClosureBodyRef::Stmt(*stmt),
                ArrowBody::Expr(value) => ClosureBodyRef::Expr(*value),
            },
            _ => return None,
        };
        Some(self.closure_assigned_symbols(body, bind))
    }

    fn forget_effects(&mut self, rec: &Recorder, effects: &ClosureEffects, bind: &BindResult) {
        for id in &effects.vars {
            self.forget_symbol(rec, *id, bind);
        }
        for key in &effects.members {
            if self.member_narrowed.remove(key).is_some() {
                self.mark_infer_env_dirty();
            }
        }
    }

    pub(crate) fn forget_closure_args(
        &mut self,
        rec: &Recorder,
        args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) {
        for arg in args {
            let value = match arg {
                varn_core::ast::Arg::Positional(value)
                | varn_core::ast::Arg::Named { value, .. } => *value,
                varn_core::ast::Arg::Spread(_) => continue,
            };
            if let Some(effects) = self.closure_def_effects(value, bind) {
                self.forget_effects(rec, &effects, bind);
            }
        }
    }

    pub(crate) fn forget_called_closure(
        &mut self,
        rec: &Recorder,
        callee: ExprId,
        callee_sid: Option<SymbolId>,
        args: &[varn_core::ast::Arg],
        bind: &BindResult,
    ) {
        if let Some(sid) = callee_sid {
            if let Some(effects) = self.closure_effects.get(&sid).cloned() {
                self.forget_effects(rec, &effects, bind);
            }
        } else if let Some(effects) = self.closure_def_effects(callee, bind) {
            self.forget_effects(rec, &effects, bind);
        }
        self.forget_closure_args(rec, args, bind);
    }

    pub(crate) fn pattern_bound_atoms(pattern: &Pattern, out: &mut Vec<Atom>) {
        match pattern {
            Pattern::Identifier { name, .. } => out.push(*name),
            Pattern::Array {
                elements, rest, ..
            } => {
                for el in elements.iter().flatten() {
                    Self::pattern_bound_atoms(&el.pattern, out);
                }
                if let Some(rest) = rest {
                    Self::pattern_bound_atoms(rest, out);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    Self::pattern_bound_atoms(&prop.value, out);
                }
                if let Some(rest) = rest {
                    Self::pattern_bound_atoms(rest, out);
                }
            }
            Pattern::Assignment { left, .. } => Self::pattern_bound_atoms(left, out),
            Pattern::Rest { argument, .. } => Self::pattern_bound_atoms(argument, out),
        }
    }
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

pub(crate) enum MemberRoot {
    This(ExprId),
    Name(ExprId, Atom),
}

pub(crate) struct AssignedTargets {
    pub vars: FxHashSet<Atom>,
    pub members: Vec<(MemberRoot, Vec<Atom>)>,
}

impl AssignedTargets {
    pub(crate) fn empty() -> Self {
        AssignedTargets {
            vars: FxHashSet::default(),
            members: Vec::new(),
        }
    }
}

pub(crate) fn assigned_in_stmt(stmt: StmtId, arena: &AstArena, out: &mut AssignedTargets) {
    match &arena.stmt(stmt).kind {
        StmtKind::Block { stmts } => {
            for s in stmts {
                assigned_in_stmt(*s, arena, out);
            }
        }
        StmtKind::Expr { expression } => assigned_in_expr(*expression, arena, out),
        StmtKind::Decl(decl) => match decl.as_ref() {
            Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Some(init) = d.init {
                        assigned_in_expr(init, arena, out);
                    }
                }
            }
            Decl::Function(_)
            | Decl::Class(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Namespace(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        },
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            assigned_in_expr(*test, arena, out);
            assigned_in_stmt(*consequent, arena, out);
            if let Some(alt) = alternate {
                assigned_in_stmt(*alt, arena, out);
            }
        }
        StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
            assigned_in_expr(*test, arena, out);
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            match init.as_deref() {
                Some(ForInit::Var { declarators, .. }) => {
                    for d in declarators {
                        if let Some(init) = d.init {
                            assigned_in_expr(init, arena, out);
                        }
                    }
                }
                Some(ForInit::Expr(e)) => assigned_in_expr(*e, arena, out),
                None => {}
            }
            for e in [test, update].into_iter().flatten() {
                assigned_in_expr(*e, arena, out);
            }
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
            assigned_in_expr(*right, arena, out);
            assigned_in_stmt(*body, arena, out);
        }
        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            assigned_in_expr(*discriminant, arena, out);
            for case in cases {
                if let Some(test) = case.test {
                    assigned_in_expr(test, arena, out);
                }
                for s in &case.body {
                    assigned_in_stmt(*s, arena, out);
                }
            }
        }
        StmtKind::Return { argument } => {
            if let Some(a) = argument {
                assigned_in_expr(*a, arena, out);
            }
        }
        StmtKind::Throw { argument } => assigned_in_expr(*argument, arena, out),
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            assigned_in_stmt(*block, arena, out);
            for c in catches {
                assigned_in_stmt(c.body, arena, out);
            }
            if let Some(f) = finally {
                assigned_in_stmt(*f, arena, out);
            }
        }
        StmtKind::Using { declarations, .. } => {
            for d in declarations {
                if let Some(init) = d.init {
                    assigned_in_expr(init, arena, out);
                }
            }
        }
        StmtKind::Labeled { body, .. } => assigned_in_stmt(*body, arena, out),
        StmtKind::Empty
        | StmtKind::Error
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Debugger => {}
    }
}

pub(crate) fn assigned_in_expr(expr: ExprId, arena: &AstArena, out: &mut AssignedTargets) {
    if let ExprKind::Assign { target, .. } = &arena.expr(expr).kind {
        if let ExprKind::Identifier { name } = &arena.expr(*target).kind {
            out.vars.insert(*name);
        } else if let Some((root, path)) = static_path_atoms(*target, arena) {
            out.members.push((root, path));
        }
    }
    walk_expr_children(expr, arena, &mut |child| assigned_in_expr(child, arena, out));
}

pub(crate) fn static_path_atoms(expr: ExprId, arena: &AstArena) -> Option<(MemberRoot, Vec<Atom>)> {
    let mut names = Vec::new();
    let mut node = expr;
    let root = loop {
        match &arena.expr(node).kind {
            ExprKind::Member {
                object,
                property,
                computed: false,
                optional: false,
                ..
            } => {
                let ExprKind::Identifier { name } = &arena.expr(*property).kind else {
                    return None;
                };
                names.push(*name);
                node = *object;
            }
            ExprKind::Identifier { name } => {
                break MemberRoot::Name(node, *name);
            }
            ExprKind::This => break MemberRoot::This(node),
            _ => return None,
        }
    };
    names.reverse();
    Some((root, names))
}
