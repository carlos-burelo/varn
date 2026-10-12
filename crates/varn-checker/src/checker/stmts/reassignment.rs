use super::super::Checker;
use crate::checker::definite_assignment::walk_expr_children;
use crate::checker::recorder::Recorder;
use rustc_hash::FxHashSet;
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
        rec: &Recorder,
        target: ExprId,
        bind: &BindResult,
    ) {
        if let ExprKind::Identifier { name } = &self.ast_arena.expr(target).kind {
            self.forget_narrowing(rec, *name, bind);
        }
    }

    pub(crate) fn forget_loop_assignments(
        &mut self,
        rec: &Recorder,
        stmt: StmtId,
        bind: &BindResult,
    ) {
        let mut names = FxHashSet::default();
        assigned_in_stmt(stmt, self.ast_arena, &mut names);
        for name in names {
            self.forget_narrowing(rec, name, bind);
        }
    }
}

pub(crate) enum ClosureBodyRef {
    Stmt(StmtId),
    Expr(ExprId),
}

impl<'r> Checker<'r> {
    pub(crate) fn closure_assigned_symbols(
        &mut self,
        body: ClosureBodyRef,
        bind: &BindResult,
    ) -> Vec<SymbolId> {
        let mut names = FxHashSet::default();
        match body {
            ClosureBodyRef::Stmt(stmt) => assigned_in_stmt(stmt, self.ast_arena, &mut names),
            ClosureBodyRef::Expr(expr) => assigned_in_expr(expr, self.ast_arena, &mut names),
        }
        let mut ids: Vec<SymbolId> = names
            .into_iter()
            .filter_map(|name| {
                bind.scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
            })
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub(crate) fn closure_def_effects(
        &mut self,
        expr: ExprId,
        bind: &BindResult,
    ) -> Option<Vec<SymbolId>> {
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
            if let Some(ids) = self.closure_def_effects(value, bind) {
                for id in &ids {
                    self.forget_symbol(rec, *id, bind);
                }
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
            if let Some(ids) = self.closure_var_effects.get(&sid).cloned() {
                for id in ids {
                    self.forget_symbol(rec, id, bind);
                }
            }
        } else if let Some(ids) = self.closure_def_effects(callee, bind) {
            for id in &ids {
                self.forget_symbol(rec, *id, bind);
            }
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

pub(crate) fn assigned_in_stmt(
    stmt: StmtId,
    arena: &AstArena,
    out: &mut FxHashSet<Atom>,
) {
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

pub(crate) fn assigned_in_expr(expr: ExprId, arena: &AstArena, out: &mut FxHashSet<Atom>) {
    if let ExprKind::Assign { target, .. } = &arena.expr(expr).kind {
        if let ExprKind::Identifier { name } = &arena.expr(*target).kind {
            out.insert(*name);
        }
    }
    walk_expr_children(expr, arena, &mut |child| assigned_in_expr(child, arena, out));
}
