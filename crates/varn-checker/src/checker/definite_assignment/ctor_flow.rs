use super::patterns::walk_expr_children;
use rustc_hash::FxHashSet;
use varn_core::ast::{AssignOp, AstArena, Decl, ExprId, ExprKind, ForInit, StmtId, StmtKind};
use varn_core::source::SourceRange;
use varn_core::Atom;

type Assigned = FxHashSet<Atom>;

pub(super) struct CtorFlow<'a> {
    arena: &'a AstArena,
    required: &'a FxHashSet<Atom>,
    pub(super) unassigned_at_exit: FxHashSet<Atom>,
    pub(super) early_reads: Vec<(Atom, SourceRange)>,
}

impl<'a> CtorFlow<'a> {
    pub(super) fn new(arena: &'a AstArena, required: &'a FxHashSet<Atom>) -> Self {
        CtorFlow {
            arena,
            required,
            unassigned_at_exit: FxHashSet::default(),
            early_reads: Vec::new(),
        }
    }

    pub(super) fn run(&mut self, body: StmtId) {
        let mut assigned = Assigned::default();
        if !self.stmt(body, &mut assigned) {
            self.exit(&assigned);
        }
    }

    fn exit(&mut self, assigned: &Assigned) {
        for field in self.required {
            if !assigned.contains(field) {
                self.unassigned_at_exit.insert(*field);
            }
        }
    }

    fn this_field(&self, e: ExprId) -> Option<Atom> {
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &self.arena.expr(e).kind
        else {
            return None;
        };
        if !matches!(self.arena.expr(*object).kind, ExprKind::This) {
            return None;
        }
        match &self.arena.expr(*property).kind {
            ExprKind::Identifier { name } => Some(*name),
            _ => None,
        }
    }

    fn expr(&mut self, e: ExprId, assigned: &mut Assigned) {
        let arena = self.arena;
        if let Some(field) = self.this_field(e) {
            if self.required.contains(&field) && !assigned.contains(&field) {
                self.early_reads.push((field, arena.expr(e).range));
            }
            return;
        }
        match &arena.expr(e).kind {
            ExprKind::Assign { op, target, value } => {
                self.expr(*value, assigned);
                match (op, self.this_field(*target)) {
                    (AssignOp::Assign, Some(field)) => {
                        assigned.insert(field);
                    }
                    _ => self.expr(*target, assigned),
                }
            }
            ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::ClassExpr { .. } => {}
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expr(*test, assigned);
                let mut then_branch = assigned.clone();
                self.expr(*consequent, &mut then_branch);
                let mut else_branch = assigned.clone();
                self.expr(*alternate, &mut else_branch);
                assigned.extend(then_branch.intersection(&else_branch).copied());
            }
            ExprKind::Logical { left, right, .. } => {
                self.expr(*left, assigned);
                self.expr(*right, &mut assigned.clone());
            }
            ExprKind::Match { .. } | ExprKind::Try { .. } | ExprKind::Pipeline { .. } => {
                let mut scratch = assigned.clone();
                walk_expr_children(e, arena, &mut |c| self.expr(c, &mut scratch));
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => {
                walk_expr_children(e, arena, &mut |c| self.expr(c, assigned))
            }
        }
    }

    fn block(&mut self, stmts: &[StmtId], assigned: &mut Assigned) -> bool {
        for &s in stmts {
            if self.stmt(s, assigned) {
                return true;
            }
        }
        false
    }

    fn branch(&mut self, s: StmtId, from: &Assigned) -> (Assigned, bool) {
        let mut assigned = from.clone();
        let diverged = self.stmt(s, &mut assigned);
        (assigned, diverged)
    }

    fn join(into: &mut Assigned, arms: Vec<(Assigned, bool)>) -> bool {
        let mut live = arms.into_iter().filter(|(_, diverged)| !diverged);
        let Some((first, _)) = live.next() else {
            return true;
        };
        let merged = live.fold(first, |acc, (set, _)| {
            acc.intersection(&set).copied().collect()
        });
        into.extend(merged);
        false
    }

    fn stmt(&mut self, s: StmtId, assigned: &mut Assigned) -> bool {
        let arena = self.arena;
        match &arena.stmt(s).kind {
            StmtKind::Block { stmts } => self.block(stmts, assigned),
            StmtKind::Expr { expression } => {
                self.expr(*expression, assigned);
                false
            }
            StmtKind::Decl(decl) => {
                if let Decl::Variable(v) = decl.as_ref() {
                    for d in &v.declarators {
                        if let Some(init) = d.init {
                            self.expr(init, assigned);
                        }
                    }
                }
                false
            }
            StmtKind::Return { argument } => {
                if let Some(a) = argument {
                    self.expr(*a, assigned);
                }
                self.exit(assigned);
                true
            }
            StmtKind::Throw { argument } => {
                self.expr(*argument, assigned);
                true
            }
            StmtKind::Break { .. } | StmtKind::Continue { .. } => true,
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.expr(*test, assigned);
                let then_arm = self.branch(*consequent, assigned);
                let else_arm = match alternate {
                    Some(alt) => self.branch(*alt, assigned),
                    None => (assigned.clone(), false),
                };
                Self::join(assigned, vec![then_arm, else_arm])
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
                self.expr(*test, assigned);
                self.branch(*body, assigned);
                false
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                match init.as_deref() {
                    Some(ForInit::Expr(e)) => self.expr(*e, assigned),
                    Some(ForInit::Var { declarators, .. }) => {
                        for d in declarators {
                            if let Some(e) = d.init {
                                self.expr(e, assigned);
                            }
                        }
                    }
                    None => {}
                }
                if let Some(t) = test {
                    self.expr(*t, assigned);
                }
                let (mut inner, _) = self.branch(*body, assigned);
                if let Some(u) = update {
                    self.expr(*u, &mut inner);
                }
                false
            }
            StmtKind::ForIn { right, body, .. } | StmtKind::ForOf { right, body, .. } => {
                self.expr(*right, assigned);
                self.branch(*body, assigned);
                false
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.expr(*discriminant, assigned);
                let mut arms = Vec::with_capacity(cases.len());
                let mut has_default = false;
                for case in cases {
                    match case.test {
                        Some(t) => self.expr(t, assigned),
                        None => has_default = true,
                    }
                    let mut arm = assigned.clone();
                    let diverged = self.block(&case.body, &mut arm);
                    arms.push((arm, diverged));
                }
                if has_default {
                    Self::join(assigned, arms)
                } else {
                    false
                }
            }
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                let mut arms = vec![self.branch(*block, assigned)];
                for clause in catches {
                    arms.push(self.branch(clause.body, assigned));
                }
                let mut diverged = Self::join(assigned, arms);
                if let Some(f) = finally
                    && self.stmt(*f, assigned)
                {
                    diverged = true;
                }
                diverged
            }
            StmtKind::Using { declarations, .. } => {
                for d in declarations {
                    if let Some(e) = d.init {
                        self.expr(e, assigned);
                    }
                }
                false
            }
            StmtKind::Labeled { body, .. } => self.stmt(*body, assigned),
            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => false,
        }
    }
}
