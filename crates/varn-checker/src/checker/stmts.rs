use super::Checker;
use crate::binder::BindResult;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::ast::{ForInit, Stmt, StmtKind};
use varn_core::{Diagnostic, ErrorCode, TypeKind, TypeTag};

impl<'r> Checker<'r> {
    pub(crate) fn check_stmts(&mut self, stmts: &[Stmt], bind: &BindResult) {
        self.check_stmts_with_guards(stmts, bind);
    }

    fn check_stmts_with_guards(&mut self, stmts: &[Stmt], bind: &BindResult) {
        let mut i = 0;
        while i < stmts.len() {
            let stmt = &stmts[i];

            if matches!(&stmt.kind, StmtKind::Throw { .. } | StmtKind::Return { .. }) {
                self.check_stmt(stmt, bind);

                for stmt in &stmts[i + 1..] {
                    self.emit(
                        Diagnostic::warning(ErrorCode::UnreachableCode, "unreachable code")
                            .with_range(stmt.range),
                    );
                }
                return;
            }

            if let Some(guard_narrowings) = self.extract_guard_narrowings(stmt, bind) {
                self.check_stmt(stmt, bind);

                self.push_narrowings(&guard_narrowings);
                self.check_stmts_with_guards(&stmts[i + 1..], bind);
                self.pop_narrowings(&guard_narrowings);
                return;
            }

            self.check_stmt(stmt, bind);
            i += 1;
        }
    }

    fn extract_guard_narrowings(
        &mut self,
        stmt: &Stmt,
        bind: &BindResult,
    ) -> Option<Vec<(SymbolId, Type)>> {
        let StmtKind::If {
            test,
            consequent,
            alternate,
        } = &stmt.kind
        else {
            return None;
        };
        if alternate.is_some() {
            return None;
        }
        if !stmt_terminates(consequent) {
            return None;
        }
        if !self.can_extract_narrowings(test) {
            return None;
        }
        let narrowings = self.extract_narrowings(test, bind, false);
        if narrowings.is_empty() {
            None
        } else {
            Some(narrowings)
        }
    }

    pub(crate) fn check_stmt(&mut self, stmt: &Stmt, bind: &BindResult) {
        match &stmt.kind {
            StmtKind::Decl(decl) => self.check_decl(decl, bind),

            StmtKind::Block { stmts } => {
                self.with_next_child_scope(bind, stmt.range.start.offset, |checker| {
                    checker.check_stmts(stmts, bind)
                });
            }

            StmtKind::Expr { expression } => {
                self.check_expr(expression, bind);
            }

            StmtKind::Return { argument } => {
                if !self.in_function {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::ReturnOutsideFunction,
                            "a 'return' statement can only be used within a function body",
                        )
                        .with_range(stmt.range),
                    );
                }

                let actual = if let Some(arg) = argument {
                    let expected_ret = self.expected_return_type.clone();
                    self.with_expected(expected_ret, |c| c.check_expr(arg, bind));
                    self.infer_type(arg, bind)
                } else {
                    Type::Void
                };

                if let Some(expected) = self.expected_return_type.clone() {
                    let is_type_param = matches!(&expected.0, TypeKind::Named(n, _) if self.active_type_params.contains(n.as_ref()));
                    if !is_type_param
                        && !self.types_compatible_cached(&expected, &actual, Some(bind))
                    {
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: function is declared to return '{expected}', but returns '{actual}'"
                            ))
                            .with_range(stmt.range),
                        );
                    }
                }
            }

            StmtKind::Break { .. } => {
                if self.loop_depth == 0 && self.switch_depth == 0 {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidBreakTarget,
                            "a 'break' statement can only be used within an enclosing iteration or switch statement",
                        )
                        .with_range(stmt.range),
                    );
                }
            }

            StmtKind::Continue { .. } => {
                if self.loop_depth == 0 {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidContinueTarget,
                            "a 'continue' statement can only be used within an enclosing iteration statement",
                        )
                        .with_range(stmt.range),
                    );
                }
            }

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.check_expr(test, bind);
                if self.can_extract_narrowings(test) {
                    let narrow_true = self.extract_narrowings(test, bind, true);
                    self.with_narrowings(&narrow_true, |checker| {
                        checker.check_stmt(consequent, bind)
                    });

                    if let Some(alt) = alternate {
                        let narrow_false = self.extract_narrowings(test, bind, false);
                        self.with_narrowings(&narrow_false, |checker| {
                            checker.check_stmt(alt, bind)
                        });
                    }
                } else {
                    self.check_stmt(consequent, bind);
                    if let Some(alt) = alternate {
                        self.check_stmt(alt, bind);
                    }
                }
            }

            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                self.check_expr(test, bind);
                self.loop_depth += 1;
                if self.can_extract_narrowings(test) {
                    let narrow_true = self.extract_narrowings(test, bind, true);
                    self.with_narrowings(&narrow_true, |checker| checker.check_stmt(body, bind));
                } else {
                    self.check_stmt(body, bind);
                }
                self.loop_depth -= 1;
            }

            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                self.loop_depth += 1;
                self.with_next_child_scope(bind, body.range.start.offset, |checker| {
                    if let Some(i) = init {
                        match i.as_ref() {
                            ForInit::Var { declarators, .. } => {
                                checker.check_for_var_init(declarators, bind)
                            }
                            ForInit::Expr(e) => checker.check_expr(e, bind),
                        }
                    }
                    if let Some(t) = test {
                        checker.check_expr(t, bind);
                    }
                    if let Some(u) = update {
                        checker.check_expr(u, bind);
                    }
                    checker.check_stmt(body, bind);
                });
                self.loop_depth -= 1;
            }

            StmtKind::ForOf {
                left, right, body, ..
            } => {
                self.check_expr(right, bind);
                let right_ty = self.infer_type(right, bind);
                let elem_ty = match &right_ty.0 {
                    TypeKind::Array(inner) => (**inner).clone(),
                    TypeKind::Generic(_name, args, _) if args.len() == 1 => args[0].clone(),
                    TypeKind::Intrinsic(TypeTag::Range) => Type::Int,
                    _ => Type::Dynamic,
                };
                self.loop_depth += 1;
                self.with_next_child_scope(bind, left.range().start.offset, |checker| {
                    checker.check_pattern(left, &elem_ty, bind);
                    checker.check_stmt(body, bind);
                });
                self.loop_depth -= 1;
            }

            StmtKind::ForIn {
                left, right, body, ..
            } => {
                self.check_expr(right, bind);
                self.loop_depth += 1;
                self.with_next_child_scope(bind, left.range().start.offset, |checker| {
                    checker.check_pattern(left, &Type::Str, bind);
                    checker.check_stmt(body, bind);
                });
                self.loop_depth -= 1;
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.check_expr(discriminant, bind);
                self.switch_depth += 1;
                let mut seen_cases = rustc_hash::FxHashSet::default();
                for case in cases {
                    if let Some(t) = &case.test {
                        self.check_expr(t, bind);
                        if let Some(lit_val) = get_literal_value_key(t) {
                            if !seen_cases.insert(lit_val.clone()) {
                                self.emit(
                                    Diagnostic::error(
                                        ErrorCode::DuplicateCaseLabel,
                                        format!("duplicate case label '{}'", lit_val),
                                    )
                                    .with_range(t.range),
                                );
                            }
                        }
                    }
                    self.check_stmts(&case.body, bind);
                }
                self.switch_depth -= 1;
            }

            StmtKind::Try {
                block,
                catch,
                finally,
            } => {
                self.check_stmt(block, bind);
                if let Some(clause) = catch {
                    self.with_next_child_scope(bind, clause.body.range.start.offset, |checker| {
                        if let Some(param) = &clause.param {
                            // `throw` only accepts `Error` subclasses, so the caught
                            // value is soundly typed as the base `Error`. Accessing a
                            // subclass requires an `instanceof` narrowing.
                            let catch_ty = Type::named("Error");
                            checker.check_pattern(param, &catch_ty, bind);
                        }
                        checker.check_stmt(&clause.body, bind);
                    });
                }
                if let Some(fin) = finally {
                    self.check_stmt(fin, bind);
                }
            }

            StmtKind::Throw { argument } => {
                self.check_expr(argument, bind);
                let thrown = self.infer_type(argument, bind);
                if !self.is_throwable(&thrown, bind) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidThrowOperand,
                            format!(
                                "cannot throw a value of type `{thrown}`: thrown values must be `Error` or a subclass"
                            ),
                        )
                        .with_range(argument.range),
                    );
                }
            }

            StmtKind::Labeled { body, .. } => {
                self.check_stmt(body, bind);
            }

            StmtKind::Using {
                declarations,
                is_await,
                ..
            } => {
                let dispose_method = if *is_await { "disposeAsync" } else { "dispose" };
                let interface_name = if *is_await {
                    varn_core::well_known::ASYNC_DISPOSABLE
                } else {
                    varn_core::well_known::DISPOSABLE
                };
                for d in declarations {
                    if d.init.is_none() {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::ConstWithoutInitializer,
                                "'using' declaration must have an initializer",
                            )
                            .with_range(d.range),
                        );
                        continue;
                    }
                    let ann = d.type_ann.as_ref().or(match &d.id {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

                    let init = d.init.as_ref().unwrap();
                    self.with_expected(ann_ty_opt.clone(), |c| c.check_expr(init, bind));
                    let init_ty = self.infer_type(init, bind);

                    if !init_ty.is_dynamic()
                        && !self.member_exists_cached(&init_ty, dispose_method, bind)
                    {
                        self.emit(
                            Diagnostic::error(ErrorCode::InvalidUsingTarget, format!(
                                "type '{init_ty}' does not implement {interface_name}: missing '{dispose_method}()' method"
                            ))
                            .with_range(d.range),
                        );
                    }

                    if let Some(ann_ty) = &ann_ty_opt {
                        if !self.types_compatible_cached(ann_ty, &init_ty, Some(bind)) {
                            self.emit(
                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                    "type mismatch: declared as '{ann_ty}' but initialised with '{init_ty}'"
                                ))
                                .with_range(d.range),
                            );
                        }
                        self.check_pattern(&d.id, ann_ty, bind);
                    } else {
                        self.check_pattern(&d.id, &init_ty, bind);
                    }
                }
            }

            _ => {}
        }
    }

    fn check_for_var_init(
        &mut self,
        declarators: &[varn_core::ast::VarDeclarator],
        bind: &BindResult,
    ) {
        for declarator in declarators {
            let ann = declarator.type_ann.as_ref().or(match &declarator.id {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            });
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            if let Some(init_expr) = &declarator.init {
                self.with_expected(ann_ty_opt.clone(), |c| c.check_expr(init_expr, bind));

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(init_expr, bind);
                    let is_empty_array = init_ty.is_dynamic()
                        && matches!(&init_expr.kind, varn_core::ast::ExprKind::Array { elements } if elements.is_empty());
                    if !is_empty_array
                        && !self.types_compatible_cached(ann_ty, &init_ty, Some(bind))
                    {
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: declared as '{ann_ty}' but initialised with '{init_ty}'"
                            ))
                            .with_range(declarator.range),
                        );
                    }
                    self.check_pattern(&declarator.id, ann_ty, bind);
                } else {
                    let init_ty = self.infer_type(init_expr, bind);
                    self.check_pattern(&declarator.id, &init_ty, bind);
                }
            }
        }
    }

    fn with_next_child_scope(&mut self, bind: &BindResult, offset: u32, f: impl FnOnce(&mut Self)) {
        let saved = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope(offset);
        }
        f(self);
        self.current_scope = saved;
    }

    pub(crate) fn with_narrowings(
        &mut self,
        narrowings: &[(crate::symbol::SymbolId, Type)],
        f: impl FnOnce(&mut Self),
    ) {
        if narrowings.is_empty() {
            f(self);
            return;
        }

        self.push_narrowings(narrowings);
        f(self);
        self.pop_narrowings(narrowings);
    }

    fn push_narrowings(&mut self, narrowings: &[(crate::symbol::SymbolId, Type)]) {
        for (id, ty) in narrowings {
            self.narrowed_types.entry(*id).or_default().push(ty.clone());
        }
        self.mark_infer_env_dirty();
    }

    fn pop_narrowings(&mut self, narrowings: &[(crate::symbol::SymbolId, Type)]) {
        for (id, _) in narrowings {
            if let Some(stack) = self.narrowed_types.get_mut(id) {
                stack.pop();
            }
        }
        self.mark_infer_env_dirty();
    }
}

fn stmt_terminates(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return { .. } | StmtKind::Throw { .. } => true,
        StmtKind::Block { stmts } => stmts.last().is_some_and(stmt_terminates),
        _ => false,
    }
}

fn get_literal_value_key(expr: &varn_core::ast::Expr) -> Option<String> {
    match &expr.kind {
        varn_core::ast::ExprKind::IntLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::FloatLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::StrLiteral { value } => Some(format!("\"{}\"", value)),
        varn_core::ast::ExprKind::BoolLiteral { value } => Some(value.to_string()),
        varn_core::ast::ExprKind::CharLiteral { value } => Some(format!("'{}'", value)),
        varn_core::ast::ExprKind::NullLiteral => Some("null".to_owned()),
        _ => None,
    }
}
