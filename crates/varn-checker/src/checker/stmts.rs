use super::Checker;
use crate::binder::BindResult;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::ast::{AstArena, ExprId, ForInit, StmtId, StmtKind};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl<'r> Checker<'r> {
    pub(crate) fn check_stmts(&mut self, stmts: &[StmtId], bind: &BindResult) {
        self.check_stmts_with_guards(stmts, bind);
    }

    fn check_stmts_with_guards(&mut self, stmts: &[StmtId], bind: &BindResult) {
        let arena = self.ast_arena;
        let mut i = 0;
        while i < stmts.len() {
            let stmt_id = stmts[i];
            let stmt = arena.stmt(stmt_id);

            if matches!(&stmt.kind, StmtKind::Throw { .. } | StmtKind::Return { .. }) {
                self.check_stmt(stmt_id, bind);

                for &later_id in &stmts[i + 1..] {
                    let later = arena.stmt(later_id);
                    self.emit(
                        Diagnostic::warning(ErrorCode::UnreachableCode, "unreachable code")
                            .with_range(later.range),
                    );
                }
                return;
            }

            if let Some(guard_narrowings) = self.extract_guard_narrowings(stmt_id, bind) {
                self.check_stmt(stmt_id, bind);

                self.push_narrowings(&guard_narrowings);
                self.check_stmts_with_guards(&stmts[i + 1..], bind);
                self.pop_narrowings(&guard_narrowings);
                return;
            }

            self.check_stmt(stmt_id, bind);
            i += 1;
        }
    }

    fn extract_guard_narrowings(
        &mut self,
        stmt: StmtId,
        bind: &BindResult,
    ) -> Option<Vec<(SymbolId, Type)>> {
        let (test, consequent, alternate) = match &self.ast_arena.stmt(stmt).kind {
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => (*test, *consequent, *alternate),
            _ => return None,
        };
        if alternate.is_some() {
            return None;
        }
        if !stmt_terminates(consequent, self.ast_arena) {
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

    pub(crate) fn check_stmt(&mut self, stmt: StmtId, bind: &BindResult) {
        let arena = self.ast_arena;
        let range = arena.stmt(stmt).range;
        match &arena.stmt(stmt).kind {
            StmtKind::Decl(decl) => {
                let decl = decl.clone();
                self.check_decl(&decl, bind);
            }

            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.with_next_child_scope_span(
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker| checker.check_stmts(&stmts, bind),
                );
            }

            StmtKind::Expr { expression } => {
                let expression = *expression;
                self.check_expr(expression, bind);
            }

            StmtKind::Return { argument } => {
                let argument = *argument;
                if !self.in_function {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::ReturnOutsideFunction,
                            "a 'return' statement can only be used within a function body",
                        )
                        .with_range(range),
                    );
                }

                let actual = if let Some(arg) = argument {
                    let expected_ret = self.expected_return_type.clone();
                    self.with_expected(expected_ret, |c| c.check_expr(arg, bind));
                    self.infer_type(arg, bind)
                } else {
                    Type::Void
                };

                if let Some(expected) = self.expected_return_type {
                    let expected_kind = self.ty_table.get(expected.0);
                    let check_expected = if matches!(expected_kind, TypeKind::TypePredicate { .. })
                    {
                        Type::Bool
                    } else {
                        expected
                    };
                    let check_expected_kind = self.ty_table.get(check_expected.0);
                    let is_type_param = matches!(check_expected_kind, TypeKind::Named(n, _) if self.active_type_params.contains(bind.interner.resolve(n)));
                    if !is_type_param
                        && !self.value_assignable_to(&check_expected, &actual, argument, Some(bind))
                    {
                        let expected_s = expected.display(&self.ty_table, &bind.interner);
                        let actual_s = actual.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: function is declared to return '{expected_s}', but returns '{actual_s}'"
                            ))
                            .with_range(range),
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
                        .with_range(range),
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
                        .with_range(range),
                    );
                }
            }

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
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
                let (test, body) = (*test, *body);
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
                let init = init.clone();
                let (test, update, body) = (*test, *update, *body);
                self.loop_depth += 1;
                self.with_next_child_scope_span(
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker| {
                        if let Some(i) = &init {
                            match i.as_ref() {
                                ForInit::Var { declarators, .. } => {
                                    checker.check_for_var_init(declarators, bind)
                                }
                                ForInit::Expr(e) => checker.check_expr(*e, bind),
                            }
                        }
                        if let Some(t) = test {
                            checker.check_expr(t, bind);
                        }
                        if let Some(u) = update {
                            checker.check_expr(u, bind);
                        }
                        checker.check_stmt(body, bind);
                    },
                );
                self.loop_depth -= 1;
            }

            StmtKind::ForOf {
                left, right, body, ..
            } => {
                let (left, right, body) = (left.clone(), *right, *body);
                self.check_expr(right, bind);
                let right_ty = self.infer_type(right, bind);
                let right_kind = self.ty_table.get(right_ty.0);
                let elem_ty = match right_kind {
                    TypeKind::Array(inner) => Type(inner, false),
                    TypeKind::Primitive(varn_core::LangPrimitive::Str) | TypeKind::TemplateLiteral(_) => Type::Char,
                    TypeKind::Named(name, _)
                        if bind.interner.get(varn_core::IntrinsicType::Str.as_str())
                            == Some(name) =>
                    {
                        Type::Char
                    }
                    TypeKind::Generic(name, args, _)
                        if bind.interner.get(varn_core::IntrinsicType::Map.as_str())
                            == Some(name)
                            && self.ty_table.get_list(args).len() == 2 =>
                    {
                        let arg_ids = self.ty_table.get_list(args).to_vec();
                        let list =
                            std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&arg_ids);
                        Type(
                            std::sync::Arc::make_mut(&mut self.ty_table)
                                .intern(TypeKind::Tuple(list)),
                            false,
                        )
                    }
                    TypeKind::Generic(_name, args, _)
                        if self.ty_table.get_list(args).len() == 1 =>
                    {
                        Type(self.ty_table.get_list(args)[0], false)
                    }
                    TypeKind::Builtin(varn_core::BuiltinType::Range) => Type::Int,
                    _ => Type::Dynamic,
                };
                self.loop_depth += 1;
                self.with_next_child_scope_span(
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker| {
                        checker.check_pattern(&left, &elem_ty, bind);
                        checker.check_stmt(body, bind);
                    },
                );
                self.loop_depth -= 1;
            }

            StmtKind::ForIn {
                left, right, body, ..
            } => {
                let (left, right, body) = (left.clone(), *right, *body);
                self.check_expr(right, bind);
                self.loop_depth += 1;
                self.with_next_child_scope_span(
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker| {
                        checker.check_pattern(&left, &Type::Str, bind);
                        checker.check_stmt(body, bind);
                    },
                );
                self.loop_depth -= 1;
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let (discriminant, cases) = (*discriminant, cases.clone());
                self.check_expr(discriminant, bind);
                self.switch_depth += 1;
                let mut seen_cases = rustc_hash::FxHashSet::default();
                for case in &cases {
                    if let Some(t) = case.test {
                        self.check_expr(t, bind);
                        if let Some(lit_val) = get_literal_value_key(t, self.ast_arena) {
                            if !seen_cases.insert(lit_val.clone()) {
                                self.emit(
                                    Diagnostic::error(
                                        ErrorCode::DuplicateCaseLabel,
                                        format!("duplicate case label '{}'", lit_val),
                                    )
                                    .with_range(self.ast_arena.expr(t).range),
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
                catches,
                finally,
            } => {
                let (block, catches, finally) = (*block, catches.clone(), *finally);
                self.check_stmt(block, bind);
                for clause in &catches {
                    self.with_next_child_scope(
                        bind,
                        self.ast_arena.stmt(clause.body).range.start.offset,
                        |checker| {
                            if let Some(param) = &clause.param {
                                let catch_ty = if let Some(ann) = &clause.type_ann {
                                    checker.resolve_type_node_cached(ann, bind)
                                } else {
                                    Type::named(
                                        "Error",
                                        checker.resolver,
                                        &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                                    )
                                };
                                checker.check_pattern(param, &catch_ty, bind);
                            }
                            checker.check_stmt(clause.body, bind);
                        },
                    );
                }
                if let Some(fin) = finally {
                    self.check_stmt(fin, bind);
                }
            }

            StmtKind::Throw { argument } => {
                let argument = *argument;
                self.check_expr(argument, bind);
                let thrown = self.infer_type(argument, bind);
                if !self.is_throwable(&thrown, bind) {
                    let thrown_s = thrown.display(&self.ty_table, &bind.interner);
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidThrowOperand,
                            format!(
                                "cannot throw a value of type `{thrown_s}`: thrown values must be `Error` or a subclass"
                            ),
                        )
                        .with_range(self.ast_arena.expr(argument).range),
                    );
                }
            }

            StmtKind::Labeled { body, .. } => {
                let body = *body;
                self.check_stmt(body, bind);
            }

            StmtKind::Using {
                declarations,
                is_await,
                ..
            } => {
                let declarations = declarations.clone();
                let is_await = *is_await;
                let dispose_method = if is_await { "disposeAsync" } else { "dispose" };
                let interface_name = if is_await {
                    varn_core::well_known::ASYNC_DISPOSABLE
                } else {
                    varn_core::well_known::DISPOSABLE
                };
                for d in &declarations {
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

                    let init = d.init.unwrap();
                    self.with_expected(ann_ty_opt.clone(), |c| c.check_expr(init, bind));
                    let init_ty = self.infer_type(init, bind);

                    if !init_ty.is_dynamic()
                        && !self.member_exists_cached(&init_ty, dispose_method, bind)
                    {
                        let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::InvalidUsingTarget, format!(
                                "type '{init_ty_s}' does not implement {interface_name}: missing '{dispose_method}()' method"
                            ))
                            .with_range(d.range),
                        );
                    }

                    if let Some(ann_ty) = &ann_ty_opt {
                        if !self.value_assignable_to(ann_ty, &init_ty, Some(init), Some(bind)) {
                            let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                            let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                            self.emit(
                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                    "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
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

            if let Some(init_expr) = declarator.init {
                self.with_expected(ann_ty_opt.clone(), |c| c.check_expr(init_expr, bind));

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(init_expr, bind);
                    let is_empty_array = init_ty.is_dynamic()
                        && matches!(&self.ast_arena.expr(init_expr).kind, varn_core::ast::ExprKind::Array { elements } if elements.is_empty());
                    if !is_empty_array
                        && !self.value_assignable_to(ann_ty, &init_ty, Some(init_expr), Some(bind))
                    {
                        let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                        let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
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

fn stmt_terminates(stmt: StmtId, arena: &AstArena) -> bool {
    match &arena.stmt(stmt).kind {
        StmtKind::Return { .. } | StmtKind::Throw { .. } => true,
        StmtKind::Block { stmts } => stmts.last().is_some_and(|&s| stmt_terminates(s, arena)),
        _ => false,
    }
}

fn get_literal_value_key(expr: ExprId, arena: &AstArena) -> Option<String> {
    match &arena.expr(expr).kind {
        varn_core::ast::ExprKind::IntLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::FloatLiteral { value, .. } => Some(value.to_string()),
        varn_core::ast::ExprKind::StrLiteral { value } => Some(format!("\"{}\"", value)),
        varn_core::ast::ExprKind::BoolLiteral { value } => Some(value.to_string()),
        varn_core::ast::ExprKind::CharLiteral { value } => Some(format!("'{}'", value)),
        varn_core::ast::ExprKind::NullLiteral => Some("null".to_owned()),
        _ => None,
    }
}
