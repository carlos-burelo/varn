use std::collections::HashSet;
use varn_core::ast::{Expr, ExprKind, Program, Stmt, StmtKind, Arg, ArrowBody, Pattern, Decl};

pub struct EscapeAnalysis {
    non_escaping_closures: HashSet<u32>, // AstIds of closures that do not escape
}

impl EscapeAnalysis {
    pub fn analyze(program: &Program) -> Self {
        let mut analysis = Self {
            non_escaping_closures: HashSet::new(),
        };
        analysis.analyze_program(program);
        analysis
    }

    pub fn is_non_escaping(&self, ast_id: u32) -> bool {
        self.non_escaping_closures.contains(&ast_id)
    }

    fn analyze_program(&mut self, program: &Program) {
        for stmt in &program.body {
            self.analyze_stmt(stmt);
        }
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.analyze_stmt(s);
                }
            }
            StmtKind::Expr { expression } => {
                self.analyze_expr(expression, false);
            }
            StmtKind::If { test, consequent, alternate } => {
                self.analyze_expr(test, false);
                self.analyze_stmt(consequent);
                if let Some(alt) = alternate {
                    self.analyze_stmt(alt);
                }
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                self.analyze_expr(test, false);
                self.analyze_stmt(body);
            }
            StmtKind::For { init, test, update, body } => {
                if let Some(init_box) = init {
                    match init_box.as_ref() {
                        varn_core::ast::ForInit::Expr(e) => {
                            self.analyze_expr(e, false);
                        }
                        varn_core::ast::ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                if let Some(init_expr) = &d.init {
                                    self.analyze_expr(init_expr, false);
                                }
                            }
                        }
                    }
                }
                if let Some(t) = test {
                    self.analyze_expr(t, false);
                }
                if let Some(u) = update {
                    self.analyze_expr(u, false);
                }
                self.analyze_stmt(body);
            }
            StmtKind::Decl(decl) => {
                self.analyze_decl(decl);
            }
            StmtKind::Return { argument } => {
                if let Some(arg) = argument {
                    // Returned expressions always escape
                    self.analyze_expr(arg, true);
                }
            }
            StmtKind::Throw { argument } => {
                self.analyze_expr(argument, true);
            }
            _ => {}
        }
    }

    fn analyze_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Variable(v) => {
                for d in &v.declarators {
                    if let Some(init) = &d.init {
                        // Let's check if the initializer is a closure and is assigned to a local variable
                        // that doesn't escape.
                        let mut escapes = true;
                        if let Pattern::Identifier { name, .. } = &d.id {
                            // If it's a local closure, analyze it.
                            escapes = self.check_local_var_escapes(name, v.range.start.offset);
                        }
                        self.analyze_expr(init, escapes);
                    }
                }
            }
            Decl::Function(f) => {
                self.analyze_stmt(&f.body);
            }
            Decl::Class(c) => {
                for member in &c.body {
                    match member {
                        varn_core::ast::ClassMember::Method { body: Some(body), .. } => {
                            self.analyze_stmt(body);
                        }
                        varn_core::ast::ClassMember::Constructor { body, .. } => {
                            self.analyze_stmt(body);
                        }
                        varn_core::ast::ClassMember::Property { init: Some(init), .. } => {
                            self.analyze_expr(init, true);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn check_local_var_escapes(&self, _name: &str, _scope_offset: u32) -> bool {
        // Conservative default: assume it escapes.
        // We will refine this if needed.
        true
    }

    fn analyze_expr(&mut self, expr: &Expr, escapes: bool) {
        match &expr.kind {
            ExprKind::Function { body, .. } => {
                if !escapes {
                    self.non_escaping_closures.insert(expr.id);
                }
                self.analyze_stmt(body);
            }
            ExprKind::Arrow { body, .. } => {
                if !escapes {
                    self.non_escaping_closures.insert(expr.id);
                }
                match body.as_ref() {
                    ArrowBody::Expr(e) => self.analyze_expr(e, true),
                    ArrowBody::Block(s) => self.analyze_stmt(s),
                }
            }
            ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
                self.analyze_expr(left, true);
                self.analyze_expr(right, true);
            }
            ExprKind::Unary { operand, .. } => {
                self.analyze_expr(operand, escapes);
            }
            ExprKind::Paren { expression, .. } => {
                self.analyze_expr(expression, escapes);
            }
            ExprKind::Assign { target, value, .. } => {
                self.analyze_expr(target, true);
                self.analyze_expr(value, true);
            }
            ExprKind::Conditional { test, consequent, alternate } => {
                self.analyze_expr(test, true);
                self.analyze_expr(consequent, escapes);
                self.analyze_expr(alternate, escapes);
            }
            ExprKind::Call { callee, args, .. } => {
                // If callee is a closure itself, e.g. (() => {})(), it doesn't escape.
                self.analyze_expr(callee, false);

                // Check if the callee is a method on an object, like array.map(...)
                let mut is_safe_method = false;
                if let ExprKind::Member { property, computed: false, .. } = &callee.kind {
                    if let ExprKind::Identifier { name } = &property.kind {
                        is_safe_method = matches!(
                            name.as_ref(),
                            "map"
                                | "filter"
                                | "forEach"
                                | "reduce"
                                | "some"
                                | "every"
                                | "find"
                                | "findIndex"
                                | "flatMap"
                                | "sort"
                                | "catch"
                                | "then"
                        );
                    }
                }

                for arg in args {
                    let arg_expr = match arg {
                        Arg::Positional(e) | Arg::Spread(e) => e,
                        Arg::Named { value, .. } => value,
                    };
                    self.analyze_expr(arg_expr, !is_safe_method);
                }
            }
            ExprKind::Array { elements } => {
                for el in elements {
                    if let varn_core::ast::ArrayEl::Expr(e) = el {
                        self.analyze_expr(e, true);
                    }
                }
            }
            ExprKind::Object { properties } => {
                for prop in properties {
                    match prop {
                        varn_core::ast::ObjectProp::Property { value, .. } => {
                            self.analyze_expr(value, true);
                        }
                        varn_core::ast::ObjectProp::Method { body, .. } => {
                            self.analyze_stmt(body);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
