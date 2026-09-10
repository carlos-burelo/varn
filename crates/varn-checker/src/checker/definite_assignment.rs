//! Definite-assignment analysis (D-2 of the TIR redesign).
//!
//! A local declared without an initializer and then read on a path where no
//! assignment reached it is an error: the interpreter returns `null` there and
//! the JIT returns `0`, so "what an unassigned binding is worth" was undefined
//! behaviour. The analysis is deliberately shallow — it tracks straight-line
//! flow, two-armed `if`, `match` and `switch`, and treats loop bodies as
//! "may not run". It never looks inside a nested function or closure.

use super::Checker;
use rustc_hash::FxHashSet;
use std::rc::Rc;
use varn_core::ast::{ArrowBody, Decl, Expr, ExprKind, MatchBody, Program, Stmt, StmtKind};
use varn_core::{Diagnostic, ErrorCode};

#[derive(Clone, Default)]
struct Flow {
    /// Locals declared without an initializer and still in scope.
    pending: FxHashSet<Rc<str>>,
    /// Locals definitely assigned at this point.
    assigned: FxHashSet<Rc<str>>,
    /// Every path to here has already left the block (`return` / `throw` /
    /// `break` / `continue`).
    diverged: bool,
}

impl Flow {
    fn assign(&mut self, name: &Rc<str>) {
        self.assigned.insert(name.clone());
    }
    /// Merge two branch outcomes back into `self` (the pre-branch state).
    fn merge(&mut self, a: Flow, b: Flow) {
        match (a.diverged, b.diverged) {
            (true, true) => self.diverged = true,
            (true, false) => *self = b,
            (false, true) => *self = a,
            (false, false) => {
                let both: FxHashSet<Rc<str>> =
                    a.assigned.intersection(&b.assigned).cloned().collect();
                self.assigned.extend(both);
            }
        }
    }
}

impl<'r> Checker<'r> {
    pub(crate) fn check_definite_assignment(&mut self, program: &Program) {
        let mut flow = Flow::default();
        self.da_block(&program.body, &mut flow);
    }

    fn da_block(&mut self, stmts: &[Stmt], flow: &mut Flow) {
        let outer_pending: FxHashSet<Rc<str>> = flow.pending.clone();
        for s in stmts {
            if flow.diverged {
                break;
            }
            self.da_stmt(s, flow);
        }
        // Locals declared in this block leave scope.
        flow.pending.retain(|n| outer_pending.contains(n));
    }

    fn da_stmt(&mut self, stmt: &Stmt, flow: &mut Flow) {
        match &stmt.kind {
            StmtKind::Block { stmts } => self.da_block(stmts, flow),
            StmtKind::Expr { expression } => self.da_expr(expression, flow),

            StmtKind::Decl(decl) => {
                if let Decl::Variable(v) = decl.as_ref() {
                    for d in &v.declarators {
                        if let Some(init) = &d.init {
                            self.da_expr(init, flow);
                        }
                        for name in pattern_names(&d.id) {
                            if d.init.is_some() {
                                flow.assigned.insert(name.clone());
                                flow.pending.remove(&name);
                            } else {
                                flow.pending.insert(name.clone());
                                flow.assigned.remove(&name);
                            }
                        }
                    }
                }
                // Function / class / enum declarations: analysed on their own.
                self.da_nested_decl(decl);
            }

            StmtKind::Return { argument } => {
                if let Some(a) = argument {
                    self.da_expr(a, flow);
                }
                flow.diverged = true;
            }
            StmtKind::Throw { argument } => {
                self.da_expr(argument, flow);
                flow.diverged = true;
            }
            StmtKind::Break { .. } | StmtKind::Continue { .. } => flow.diverged = true,

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.da_expr(test, flow);
                let mut a = flow.clone();
                self.da_stmt(consequent, &mut a);
                let mut b = flow.clone();
                if let Some(alt) = alternate {
                    self.da_stmt(alt, &mut b);
                }
                flow.merge(a, b);
            }

            StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
                self.da_expr(test, flow);
                let mut inner = flow.clone();
                inner.diverged = false;
                self.da_stmt(body, &mut inner);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                if let Some(fi) = init {
                    match fi.as_ref() {
                        varn_core::ast::ForInit::Expr(e) => self.da_expr(e, flow),
                        varn_core::ast::ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                if let Some(e) = &d.init {
                                    self.da_expr(e, flow);
                                }
                                for n in pattern_names(&d.id) {
                                    flow.assigned.insert(n);
                                }
                            }
                        }
                    }
                }
                if let Some(t) = test {
                    self.da_expr(t, flow);
                }
                let mut inner = flow.clone();
                inner.diverged = false;
                self.da_stmt(body, &mut inner);
                if let Some(u) = update {
                    self.da_expr(u, &mut inner);
                }
            }
            StmtKind::ForIn {
                right, body, left, ..
            }
            | StmtKind::ForOf {
                right, body, left, ..
            } => {
                self.da_expr(right, flow);
                let mut inner = flow.clone();
                inner.diverged = false;
                for n in pattern_names(left) {
                    inner.assigned.insert(n);
                }
                self.da_stmt(body, &mut inner);
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.da_expr(discriminant, flow);
                let mut acc: Option<Flow> = None;
                let mut has_default = false;
                for c in cases {
                    if let Some(t) = &c.test {
                        self.da_expr(t, flow);
                    } else {
                        has_default = true;
                    }
                    let mut arm = flow.clone();
                    for s in &c.body {
                        if arm.diverged {
                            break;
                        }
                        self.da_stmt(s, &mut arm);
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
                if has_default {
                    if let Some(a) = acc {
                        *flow = a;
                    }
                }
            }

            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                let mut t = flow.clone();
                self.da_stmt(block, &mut t);
                let mut c = flow.clone();
                if let Some(clause) = catches.first() {
                    self.da_stmt(&clause.body, &mut c);
                }
                flow.merge(t, c);
                if let Some(f) = finally {
                    self.da_stmt(f, flow);
                }
            }

            StmtKind::Using { declarations, .. } => {
                for d in declarations {
                    if let Some(e) = &d.init {
                        self.da_expr(e, flow);
                    }
                    for n in pattern_names(&d.id) {
                        flow.assigned.insert(n);
                    }
                }
            }
            StmtKind::Labeled { body, .. } => self.da_stmt(body, flow),

            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => {}
        }
    }

    fn da_expr(&mut self, e: &Expr, flow: &mut Flow) {
        match &e.kind {
            ExprKind::Identifier { name } => {
                if flow.pending.contains(name) && !flow.assigned.contains(name) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::UseBeforeAssignment,
                            format!("'{name}' is used before it is assigned a value"),
                        )
                        .with_range(*e.range()),
                    );
                    // Report once per binding.
                    flow.assigned.insert(name.clone());
                }
            }

            // An assignment to a bare identifier makes it assigned; the value
            // is analysed first.
            ExprKind::Assign { target, value, .. } => {
                self.da_expr(value, flow);
                if let ExprKind::Identifier { name } = &target.kind {
                    flow.assign(name);
                } else {
                    self.da_expr(target, flow);
                }
            }

            // Nested functions have their own flow; do not walk into them here.
            ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::ClassExpr { .. } => {}

            _ => walk_expr_children(e, &mut |c| self.da_expr(c, flow)),
        }
    }

    fn da_nested_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function(f) => {
                let mut flow = Flow::default();
                self.da_stmt(&f.body, &mut flow);
            }
            Decl::Class(c) => {
                for m in &c.body {
                    use varn_core::ast::ClassMember;
                    let body = match m {
                        ClassMember::Method { body: Some(b), .. }
                        | ClassMember::Getter { body: Some(b), .. }
                        | ClassMember::Setter { body: Some(b), .. } => Some(b),
                        ClassMember::Constructor { body, .. }
                        | ClassMember::StaticBlock { body, .. } => Some(body),
                        _ => None,
                    };
                    if let Some(b) = body {
                        let mut flow = Flow::default();
                        self.da_stmt(b, &mut flow);
                    }
                }
            }
            Decl::Export(varn_core::ast::ExportDecl::Decl { declaration, .. }) => {
                self.da_nested_decl(declaration);
            }
            _ => {}
        }
    }
}

fn pattern_names(p: &varn_core::ast::Pattern) -> Vec<Rc<str>> {
    use varn_core::ast::Pattern;
    let mut out = Vec::new();
    fn go(p: &Pattern, out: &mut Vec<Rc<str>>) {
        match p {
            Pattern::Identifier { name, .. } => out.push(name.clone()),
            Pattern::Array { elements, rest, .. } => {
                for e in elements.iter().flatten() {
                    go(&e.pattern, out);
                }
                if let Some(r) = rest {
                    go(r, out);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for pr in properties {
                    go(&pr.value, out);
                }
                if let Some(r) = rest {
                    go(r, out);
                }
            }
            Pattern::Assignment { left, .. } => go(left, out),
            Pattern::Rest { argument, .. } => go(argument, out),
        }
    }
    go(p, &mut out);
    out
}

/// Apply `f` to every direct sub-expression of `e`. Deliberately structural —
/// it does not need to know what each node means, only where the children are.
fn walk_expr_children(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    use varn_core::ast::{Arg, ArrayEl, ObjectProp, TemplatePart};
    match &e.kind {
        ExprKind::Template { parts } => {
            for p in parts {
                if let TemplatePart::Interpolation(x) = p {
                    f(x);
                }
            }
        }
        ExprKind::TaggedTemplate { tag, template } => {
            f(tag);
            f(template);
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(x) | ArrayEl::Spread(x) => f(x),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
            for p in properties {
                match p {
                    ObjectProp::Property { value, .. } => f(value),
                    ObjectProp::Spread { argument, .. } => f(argument),
                    _ => {}
                }
            }
        }
        ExprKind::Tuple { elements } => elements.iter().for_each(&mut *f),
        ExprKind::Unary { operand, .. }
        | ExprKind::Update { operand, .. }
        | ExprKind::Paren {
            expression: operand,
        }
        | ExprKind::Await { argument: operand }
        | ExprKind::Spawn { argument: operand }
        | ExprKind::NonNull {
            expression: operand,
        }
        | ExprKind::Try {
            expression: operand,
        }
        | ExprKind::As {
            expression: operand,
            ..
        }
        | ExprKind::Satisfies {
            expression: operand,
            ..
        }
        | ExprKind::Is {
            expression: operand,
            ..
        } => f(operand),
        ExprKind::Yield { argument, .. } => {
            if let Some(x) = argument {
                f(x);
            }
        }
        ExprKind::Binary { left, right, .. }
        | ExprKind::Logical { left, right, .. }
        | ExprKind::Pipeline { left, right }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            f(left);
            f(right);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            f(test);
            f(consequent);
            f(alternate);
        }
        ExprKind::Member {
            object, property, ..
        } => {
            f(object);
            f(property);
        }
        ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
            f(callee);
            for a in args {
                match a {
                    Arg::Positional(x) | Arg::Spread(x) | Arg::Named { value: x, .. } => f(x),
                }
            }
        }
        ExprKind::Sequence { expressions } => expressions.iter().for_each(&mut *f),
        ExprKind::MetaAccess { target, .. } => f(target),
        ExprKind::With { object, properties } => {
            f(object);
            for p in properties {
                if let ObjectProp::Property { value, .. } = p {
                    f(value);
                }
            }
        }
        ExprKind::Match { subject, cases } => {
            f(subject);
            for c in cases {
                if let Some(g) = &c.guard {
                    f(g);
                }
                match &c.body {
                    MatchBody::Expr(x) => f(x),
                    MatchBody::Block(_) => {}
                }
            }
        }
        _ => {}
    }
}

/// For an arrow body used as an expression source.
#[allow(dead_code)]
fn arrow_body_expr(b: &ArrowBody) -> Option<&Expr> {
    match b {
        ArrowBody::Expr(e) => Some(e),
        ArrowBody::Block(_) => None,
    }
}
