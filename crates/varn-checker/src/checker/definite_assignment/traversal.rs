use super::flow::Flow;
use super::hoist::block_bindings;
use super::patterns::{pattern_names, walk_expr_children};
use crate::checker::Checker;
use varn_core::ast::{ClassMember, Decl, ExportDecl, ExprId, ExprKind, Program, StmtId, StmtKind};
use varn_core::{Atom, Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(crate) fn check_definite_assignment(&mut self, program: &Program, bind: &BindResult) {
        let mut flow = Flow::default();
        self.da_block(&program.body, &mut flow, bind);
    }

    fn da_expr(&mut self, e: ExprId, flow: &mut Flow, bind: &BindResult) {
        let arena = self.ast_arena;
        match &arena.expr(e).kind {
            ExprKind::Identifier { name } => self.da_read(*name, e, flow, bind),

            ExprKind::Assign { target, value, .. } => {
                let (target, value) = (*target, *value);
                self.da_expr(value, flow, bind);
                if let ExprKind::Identifier { name } = &arena.expr(target).kind {
                    if flow.undeclared.contains(name) {
                        self.da_read(*name, target, flow, bind);
                    }
                    flow.assign(name);
                } else {
                    self.da_expr(target, flow, bind);
                }
            }

            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                self.da_expr(*object, flow, bind);
                if *computed {
                    self.da_expr(*property, flow, bind);
                }
            }

            ExprKind::Call { callee, .. } => {
                if let ExprKind::Identifier { name } = &arena.expr(*callee).kind {
                    self.da_call(*name, e, flow, bind);
                }
                walk_expr_children(e, arena, &mut |c| self.da_expr(c, flow, bind))
            }

            ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::ClassExpr { .. } => {}

            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
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
            | ExprKind::Logical { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::New { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => {
                walk_expr_children(e, arena, &mut |c| self.da_expr(c, flow, bind))
            }
        }
    }

    fn da_read(&mut self, name: Atom, at: ExprId, flow: &mut Flow, bind: &BindResult) {
        let range = self.ast_arena.expr(at).range;
        let text = bind.interner.resolve(name);
        if flow.undeclared.remove(&name) {
            flow.assigned.insert(name);
            self.emit(
                Diagnostic::error(
                    ErrorCode::UseBeforeDeclaration,
                    format!("'{text}' is used before its declaration"),
                )
                .with_range(range),
            );
        } else if flow.pending.contains(&name) && !flow.assigned.contains(&name) {
            flow.assigned.insert(name);
            self.emit(
                Diagnostic::error(
                    ErrorCode::UseBeforeAssignment,
                    format!("'{text}' is used before it is assigned a value"),
                )
                .with_range(range),
            );
        }
    }

    fn da_call(&mut self, callee: Atom, at: ExprId, flow: &Flow, bind: &BindResult) {
        let Some(reads) = flow.callee_reads(&callee) else {
            return;
        };
        let mut unready: Vec<Atom> = reads.into_iter().filter(|n| flow.unready(n)).collect();
        unready.sort_by_key(|n| bind.interner.resolve(*n).to_string());
        if let Some(first) = unready.first() {
            let (callee, first) = (bind.interner.resolve(callee), bind.interner.resolve(*first));
            self.emit(
                Diagnostic::error(
                    ErrorCode::UseBeforeDeclaration,
                    format!("calling '{callee}' reads '{first}' before it is declared"),
                )
                .with_range(self.ast_arena.expr(at).range),
            );
        }
    }

    fn reject_param_redeclaration(
        &mut self,
        params: &[varn_core::ast::Param],
        body: StmtId,
        bind: &BindResult,
    ) {
        let StmtKind::Block { stmts } = &self.ast_arena.stmt(body).kind else {
            return;
        };
        let names: Vec<Atom> = params.iter().flat_map(|p| pattern_names(&p.pattern)).collect();
        for hoisted in block_bindings(self.ast_arena, stmts) {
            if hoisted.is_binding && names.contains(&hoisted.name) {
                let text = bind.interner.resolve(hoisted.name);
                self.emit(
                    Diagnostic::error(
                        ErrorCode::DuplicateDeclaration,
                        format!("'{text}' is already declared as a parameter of this function"),
                    )
                    .with_range(self.ast_arena.stmt(body).range),
                );
            }
        }
    }

    fn da_declared(&mut self, decl: &Decl, flow: &mut Flow, bind: &BindResult) {
        match decl {
            Decl::Class(c) => {
                if let Some(id) = c.id {
                    flow.declare(&id, true);
                }
                for member in &c.body {
                    match member {
                        ClassMember::Property {
                            init: Some(init),
                            modifiers,
                            ..
                        } if modifiers.is_static => self.da_expr(*init, flow, bind),
                        ClassMember::StaticBlock { body, .. } => {
                            let mut inner = flow.clone();
                            self.da_stmt(*body, &mut inner, bind);
                        }
                        ClassMember::Constructor { .. }
                        | ClassMember::Destructor { .. }
                        | ClassMember::Method { .. }
                        | ClassMember::Property { .. }
                        | ClassMember::Getter { .. }
                        | ClassMember::Setter { .. } => {}
                    }
                }
            }
            Decl::Enum(e) => flow.declare(&e.id, true),
            Decl::Export(ExportDecl::Decl { declaration, .. }) => {
                self.da_declared(declaration, flow, bind)
            }
            Decl::Variable(_)
            | Decl::Function(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Namespace(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }

    fn da_nested_decl(&mut self, decl: &Decl, bind: &BindResult) {
        match decl {
            Decl::Function(f) => {
                self.reject_param_redeclaration(&f.params, f.body, bind);
                let mut flow = Flow::default();
                self.da_stmt(f.body, &mut flow, bind);
            }
            Decl::Class(c) => {
                self.check_class_fields(c, bind);
                for m in &c.body {
                    use varn_core::ast::ClassMember;
                    let body = match m {
                        ClassMember::Method { body: Some(b), .. }
                        | ClassMember::Getter { body: Some(b), .. }
                        | ClassMember::Setter { body: Some(b), .. } => Some(*b),
                        ClassMember::Constructor { body, .. }
                        | ClassMember::StaticBlock { body, .. } => Some(*body),
                        ClassMember::Destructor { .. }
                        | ClassMember::Method { .. }
                        | ClassMember::Property { .. }
                        | ClassMember::Getter { .. }
                        | ClassMember::Setter { .. } => None,
                    };
                    if let Some(b) = body {
                        if let ClassMember::Method { params, .. }
                        | ClassMember::Constructor { params, .. } = m
                        {
                            self.reject_param_redeclaration(params, b, bind);
                        }
                        let mut flow = Flow::default();
                        self.da_stmt(b, &mut flow, bind);
                    }
                }
            }
            Decl::Export(varn_core::ast::ExportDecl::Decl { declaration, .. }) => {
                self.da_nested_decl(declaration, bind);
            }
            Decl::Namespace(ns) => {
                for inner in &ns.body {
                    self.da_nested_decl(inner, bind);
                }
            }
            Decl::Variable(_)
            | Decl::Interface(_)
            | Decl::TypeAlias(_)
            | Decl::Enum(_)
            | Decl::Import(_)
            | Decl::Export(_)
            | Decl::Extension(_)
            | Decl::Struct(_)
            | Decl::SumType(_) => {}
        }
    }

    fn da_block(&mut self, stmts: &[StmtId], flow: &mut Flow, bind: &BindResult) {
        let saved = flow.enter(block_bindings(self.ast_arena, stmts));
        for &s in stmts {
            if flow.diverged {
                break;
            }
            self.da_stmt(s, flow, bind);
        }
        flow.leave(saved);
    }

    fn da_stmt(&mut self, stmt: StmtId, flow: &mut Flow, bind: &BindResult) {
        let arena = self.ast_arena;
        match &arena.stmt(stmt).kind {
            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.da_block(&stmts, flow, bind);
            }
            StmtKind::Expr { expression } => self.da_expr(*expression, flow, bind),

            StmtKind::Decl(decl) => {
                let decl = decl.clone();
                if let Decl::Variable(v) = decl.as_ref() {
                    for d in &v.declarators {
                        if let Some(init) = d.init {
                            self.da_expr(init, flow, bind);
                        }
                        for name in pattern_names(&d.id) {
                            flow.declare(&name, d.init.is_some());
                        }
                    }
                }

                self.da_nested_decl(&decl, bind);
                self.da_declared(&decl, flow, bind);
            }

            StmtKind::Return { argument } => {
                let argument = *argument;
                if let Some(a) = argument {
                    self.da_expr(a, flow, bind);
                }
                flow.diverged = true;
            }
            StmtKind::Throw { argument } => {
                let argument = *argument;
                self.da_expr(argument, flow, bind);
                flow.diverged = true;
            }
            StmtKind::Break { .. } | StmtKind::Continue { .. } => flow.diverged = true,

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                self.da_expr(test, flow, bind);
                let mut a = flow.clone();
                self.da_stmt(consequent, &mut a, bind);
                let mut b = flow.clone();
                if let Some(alt) = alternate {
                    self.da_stmt(alt, &mut b, bind);
                }
                flow.merge(a, b);
            }

            StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
                let (test, body) = (*test, *body);
                self.da_expr(test, flow, bind);
                let mut inner = flow.clone();
                inner.diverged = false;
                self.da_stmt(body, &mut inner, bind);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let init = init.clone();
                let (test, update, body) = (*test, *update, *body);
                if let Some(fi) = &init {
                    match fi.as_ref() {
                        varn_core::ast::ForInit::Expr(e) => self.da_expr(*e, flow, bind),
                        varn_core::ast::ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                if let Some(e) = d.init {
                                    self.da_expr(e, flow, bind);
                                }
                                for n in pattern_names(&d.id) {
                                    flow.assigned.insert(n);
                                }
                            }
                        }
                    }
                }
                if let Some(t) = test {
                    self.da_expr(t, flow, bind);
                }
                let mut inner = flow.clone();
                inner.diverged = false;
                self.da_stmt(body, &mut inner, bind);
                if let Some(u) = update {
                    self.da_expr(u, &mut inner, bind);
                }
            }
            StmtKind::ForIn {
                right, body, left, ..
            }
            | StmtKind::ForOf {
                right, body, left, ..
            } => {
                let (right, body, left) = (*right, *body, left.clone());
                self.da_expr(right, flow, bind);
                let mut inner = flow.clone();
                inner.diverged = false;
                for n in pattern_names(&left) {
                    inner.assigned.insert(n);
                }
                self.da_stmt(body, &mut inner, bind);
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let (discriminant, cases) = (*discriminant, cases.clone());
                self.da_expr(discriminant, flow, bind);
                let mut acc: Option<Flow> = None;
                let mut has_default = false;
                for c in &cases {
                    if let Some(t) = c.test {
                        self.da_expr(t, flow, bind);
                    } else {
                        has_default = true;
                    }
                    let mut arm = flow.clone();
                    for &s in &c.body {
                        if arm.diverged {
                            break;
                        }
                        self.da_stmt(s, &mut arm, bind);
                    }
                    acc = Some(match acc {
                        None => arm,
                        Some(prev) => {
                            let mut m = flow.clone();
                            m.merge(prev, arm);
                            m
                        }
                    });
                }
                if has_default && let Some(a) = acc {
                    *flow = a;
                }
            }

            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                let (block, catches, finally) = (*block, catches.clone(), *finally);
                let mut t = flow.clone();
                self.da_stmt(block, &mut t, bind);
                let mut c = flow.clone();
                if let Some(clause) = catches.first() {
                    self.da_stmt(clause.body, &mut c, bind);
                }
                flow.merge(t, c);
                if let Some(f) = finally {
                    self.da_stmt(f, flow, bind);
                }
            }

            StmtKind::Using { declarations, .. } => {
                let declarations = declarations.clone();
                for d in &declarations {
                    if let Some(e) = d.init {
                        self.da_expr(e, flow, bind);
                    }
                    for n in pattern_names(&d.id) {
                        flow.assigned.insert(n);
                    }
                }
            }
            StmtKind::Labeled { body, .. } => {
                let body = *body;
                self.da_stmt(body, flow, bind);
            }

            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => {}
        }
    }
}
