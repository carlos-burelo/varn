use super::recorder::Recorder;
use super::Checker;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_function_decl(
        &mut self,
        rec: &mut Recorder,
        f: &varn_core::ast::FunctionDecl,
        bind: &BindResult,
    ) {
        let saved_expected = self.expected_return_type.take();
        self.expected_return_type = f.return_type.as_ref().map(|rt| {
            let ty = self.resolve_type_node_cached(rt, bind);
            if f.modifiers.is_async {
                varn_sem::types::awaited(&ty, &self.ty_table)
            } else {
                ty
            }
        });

        let saved_scope = self.current_scope;
        let next_scope = self.next_child_scope(bind);
        if let Some(fn_scope) = next_scope {
            self.current_scope = fn_scope;
            self.record_scope_span(rec, f.range.start.offset, f.range.end.offset, fn_scope);
        }
        let mut injected_tps = Vec::new();
        for tp in &f.type_params {
            let tp_name: Arc<str> = Arc::from(bind.interner.resolve(tp.name));
            self.active_type_params.insert(tp_name.clone());
            injected_tps.push(tp_name);
        }

        let is_gen = f.modifiers.is_generator;
        let old_yields = if is_gen {
            self.yielded_types.replace(Vec::new())
        } else {
            None
        };

        let saved_caps = self
            .enclosing_caps
            .replace(super::decorator_signature::caps_of(
                &f.decorators,
                self.ast_arena,
                bind,
            ));
        let saved_pure = self.pure_scope.take();
        if super::decorator_signature::is_pure_fn(&f.decorators, self.ast_arena, bind) {
            self.pure_scope = next_scope.or(Some(self.current_scope));
        }

        self.in_function_body(f.modifiers.is_async, |c| c.check_stmt(rec, f.body, bind));

        self.pure_scope = saved_pure;
        self.enclosing_caps = saved_caps;

        if is_gen {
            let yields = self.yielded_types.take().unwrap_or_default();
            if f.return_type.is_none() {
                let inferred_yield = if yields.is_empty() {
                    Type::Void
                } else {
                    Type::union(yields, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                };
                let scope = bind.scopes.get(saved_scope);
                if let Some(sym_id) = scope.resolve(f.id, &bind.scopes) {
                    if let Some(fn_ty) = rec
                        .symbol_types
                        .get(&sym_id)
                        .cloned()
                        .or_else(|| bind.arena.get(sym_id).ty)
                    {
                        if let varn_core::TypeKind::Fn(fid) = self.ty_table.get(fn_ty.0) {
                            let new_ret = varn_sem::types::generator_of(
                                inferred_yield,
                                f.modifiers.is_async,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            );
                            let mut ft = self.ty_table.get_function(fid).clone();
                            ft.return_type = new_ret.0;
                            let new_fn_ty =
                                Type::fn_(ft, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                            rec.symbol_types.insert(sym_id, new_fn_ty);
                            self.record_type_with_symbol(rec, f.id_offset, new_fn_ty, sym_id);
                        }
                    }
                }
            }
            self.yielded_types = old_yields;
        }

        self.current_scope = saved_scope;
        self.expected_return_type = saved_expected;
        for tp in &injected_tps {
            self.active_type_params.remove(tp.as_ref());
        }
        if !f.decorators.is_empty() {
            let name = bind.interner.resolve(f.id);
            self.check_decorator_signatures(
                rec,
                &f.decorators,
                super::decorator_signature::DecoratorTarget::Function,
                name,
                bind,
            );
            let has_test = varn_core::ast::decorators::match_builtin(
                self.ast_arena,
                &bind.interner,
                &f.decorators,
            )
            .into_iter()
            .zip(f.decorators.iter())
            .any(|(m, d)| {
                !bind.user_decorators.contains(&d.range.start.offset)
                    && matches!(
                        m.result,
                        Some(Ok(varn_core::ast::decorators::BuiltinDecorator::Test))
                    )
            });
            if has_test
                && f.params
                    .iter()
                    .any(|p| p.default.is_none() && !p.is_optional && !p.is_rest)
            {
                self.emit(
                    varn_core::diagnostics::Diagnostic::error(
                        varn_core::diagnostics::ErrorCode::InvalidDecoratorTarget,
                        format!(
                            "`@test` function '{name}' must take no required arguments (the runner calls it with none)"
                        ),
                    )
                    .with_range(f.range),
                );
            }
            if self.has_builtin(&f.decorators, bind, "inline") {
                self.check_inline_shape(f, name);
            }
        }
    }

    fn has_builtin(
        &self,
        decorators: &[varn_core::ast::Decorator],
        bind: &BindResult,
        want: &str,
    ) -> bool {
        decorators.iter().any(|d| {
            if bind.user_decorators.contains(&d.range.start.offset) {
                return false;
            }
            let expr = &self.ast_arena.expr(d.expression);
            let head = match &expr.kind {
                varn_core::ast::ExprKind::Identifier { name } => Some(*name),
                varn_core::ast::ExprKind::Call { callee, .. } => {
                    match &self.ast_arena.expr(*callee).kind {
                        varn_core::ast::ExprKind::Identifier { name } => Some(*name),
                        varn_core::ast::ExprKind::IntLiteral { .. }
                        | varn_core::ast::ExprKind::FloatLiteral { .. }
                        | varn_core::ast::ExprKind::BigIntLiteral { .. }
                        | varn_core::ast::ExprKind::DecimalLiteral { .. }
                        | varn_core::ast::ExprKind::StrLiteral { .. }
                        | varn_core::ast::ExprKind::CharLiteral { .. }
                        | varn_core::ast::ExprKind::BoolLiteral { .. }
                        | varn_core::ast::ExprKind::NullLiteral
                        | varn_core::ast::ExprKind::RegexLiteral { .. }
                        | varn_core::ast::ExprKind::Template { .. }
                        | varn_core::ast::ExprKind::TaggedTemplate { .. }
                        | varn_core::ast::ExprKind::Missing
                        | varn_core::ast::ExprKind::This
                        | varn_core::ast::ExprKind::Super
                        | varn_core::ast::ExprKind::Array { .. }
                        | varn_core::ast::ExprKind::Object { .. }
                        | varn_core::ast::ExprKind::Tuple { .. }
                        | varn_core::ast::ExprKind::Record { .. }
                        | varn_core::ast::ExprKind::Unary { .. }
                        | varn_core::ast::ExprKind::Update { .. }
                        | varn_core::ast::ExprKind::Binary { .. }
                        | varn_core::ast::ExprKind::Logical { .. }
                        | varn_core::ast::ExprKind::Assign { .. }
                        | varn_core::ast::ExprKind::Conditional { .. }
                        | varn_core::ast::ExprKind::Member { .. }
                        | varn_core::ast::ExprKind::Call { .. }
                        | varn_core::ast::ExprKind::New { .. }
                        | varn_core::ast::ExprKind::Function { .. }
                        | varn_core::ast::ExprKind::Arrow { .. }
                        | varn_core::ast::ExprKind::Sequence { .. }
                        | varn_core::ast::ExprKind::Paren { .. }
                        | varn_core::ast::ExprKind::Await { .. }
                        | varn_core::ast::ExprKind::Spawn { .. }
                        | varn_core::ast::ExprKind::Yield { .. }
                        | varn_core::ast::ExprKind::Spread { .. }
                        | varn_core::ast::ExprKind::Pipeline { .. }
                        | varn_core::ast::ExprKind::Range { .. }
                        | varn_core::ast::ExprKind::NonNull { .. }
                        | varn_core::ast::ExprKind::Try { .. }
                        | varn_core::ast::ExprKind::As { .. }
                        | varn_core::ast::ExprKind::Satisfies { .. }
                        | varn_core::ast::ExprKind::ClassExpr { .. }
                        | varn_core::ast::ExprKind::Match { .. }
                        | varn_core::ast::ExprKind::Is { .. }
                        | varn_core::ast::ExprKind::With { .. }
                        | varn_core::ast::ExprKind::MetaAccess { .. } => None,
                    }
                }
                varn_core::ast::ExprKind::IntLiteral { .. }
                | varn_core::ast::ExprKind::FloatLiteral { .. }
                | varn_core::ast::ExprKind::BigIntLiteral { .. }
                | varn_core::ast::ExprKind::DecimalLiteral { .. }
                | varn_core::ast::ExprKind::StrLiteral { .. }
                | varn_core::ast::ExprKind::CharLiteral { .. }
                | varn_core::ast::ExprKind::BoolLiteral { .. }
                | varn_core::ast::ExprKind::NullLiteral
                | varn_core::ast::ExprKind::RegexLiteral { .. }
                | varn_core::ast::ExprKind::Template { .. }
                | varn_core::ast::ExprKind::TaggedTemplate { .. }
                | varn_core::ast::ExprKind::Missing
                | varn_core::ast::ExprKind::This
                | varn_core::ast::ExprKind::Super
                | varn_core::ast::ExprKind::Array { .. }
                | varn_core::ast::ExprKind::Object { .. }
                | varn_core::ast::ExprKind::Tuple { .. }
                | varn_core::ast::ExprKind::Record { .. }
                | varn_core::ast::ExprKind::Unary { .. }
                | varn_core::ast::ExprKind::Update { .. }
                | varn_core::ast::ExprKind::Binary { .. }
                | varn_core::ast::ExprKind::Logical { .. }
                | varn_core::ast::ExprKind::Assign { .. }
                | varn_core::ast::ExprKind::Conditional { .. }
                | varn_core::ast::ExprKind::Member { .. }
                | varn_core::ast::ExprKind::New { .. }
                | varn_core::ast::ExprKind::Function { .. }
                | varn_core::ast::ExprKind::Arrow { .. }
                | varn_core::ast::ExprKind::Sequence { .. }
                | varn_core::ast::ExprKind::Paren { .. }
                | varn_core::ast::ExprKind::Await { .. }
                | varn_core::ast::ExprKind::Spawn { .. }
                | varn_core::ast::ExprKind::Yield { .. }
                | varn_core::ast::ExprKind::Spread { .. }
                | varn_core::ast::ExprKind::Pipeline { .. }
                | varn_core::ast::ExprKind::Range { .. }
                | varn_core::ast::ExprKind::NonNull { .. }
                | varn_core::ast::ExprKind::Try { .. }
                | varn_core::ast::ExprKind::As { .. }
                | varn_core::ast::ExprKind::Satisfies { .. }
                | varn_core::ast::ExprKind::ClassExpr { .. }
                | varn_core::ast::ExprKind::Match { .. }
                | varn_core::ast::ExprKind::Is { .. }
                | varn_core::ast::ExprKind::With { .. }
                | varn_core::ast::ExprKind::MetaAccess { .. } => None,
            };
            head.is_some_and(|h| bind.interner.resolve(h) == want)
        })
    }

    fn check_inline_shape(&mut self, f: &varn_core::ast::FunctionDecl, name: &str) {
        let mut reason: Option<&str> = None;
        if f.modifiers.is_async {
            reason = Some("async functions");
        } else if f.modifiers.is_generator {
            reason = Some("generators");
        } else if f.params.iter().any(|p| p.is_rest) {
            reason = Some("rest parameters");
        } else {
            match &self.ast_arena.stmt(f.body).kind {
                varn_core::ast::StmtKind::Block { stmts } if stmts.len() == 1 => {
                    match &self.ast_arena.stmt(stmts[0]).kind {
                        varn_core::ast::StmtKind::Return {
                            argument: Some(arg),
                        } => match &self.ast_arena.expr(*arg).kind {
                            varn_core::ast::ExprKind::Arrow { .. }
                            | varn_core::ast::ExprKind::Function { .. }
                            | varn_core::ast::ExprKind::ClassExpr { .. } => {
                                reason = Some("closures returned by value");
                            }
                            varn_core::ast::ExprKind::IntLiteral { .. }
                            | varn_core::ast::ExprKind::FloatLiteral { .. }
                            | varn_core::ast::ExprKind::BigIntLiteral { .. }
                            | varn_core::ast::ExprKind::DecimalLiteral { .. }
                            | varn_core::ast::ExprKind::StrLiteral { .. }
                            | varn_core::ast::ExprKind::CharLiteral { .. }
                            | varn_core::ast::ExprKind::BoolLiteral { .. }
                            | varn_core::ast::ExprKind::NullLiteral
                            | varn_core::ast::ExprKind::RegexLiteral { .. }
                            | varn_core::ast::ExprKind::Template { .. }
                            | varn_core::ast::ExprKind::TaggedTemplate { .. }
                            | varn_core::ast::ExprKind::Identifier { .. }
                            | varn_core::ast::ExprKind::Missing
                            | varn_core::ast::ExprKind::This
                            | varn_core::ast::ExprKind::Super
                            | varn_core::ast::ExprKind::Array { .. }
                            | varn_core::ast::ExprKind::Object { .. }
                            | varn_core::ast::ExprKind::Tuple { .. }
                            | varn_core::ast::ExprKind::Record { .. }
                            | varn_core::ast::ExprKind::Unary { .. }
                            | varn_core::ast::ExprKind::Update { .. }
                            | varn_core::ast::ExprKind::Binary { .. }
                            | varn_core::ast::ExprKind::Logical { .. }
                            | varn_core::ast::ExprKind::Assign { .. }
                            | varn_core::ast::ExprKind::Conditional { .. }
                            | varn_core::ast::ExprKind::Member { .. }
                            | varn_core::ast::ExprKind::Call { .. }
                            | varn_core::ast::ExprKind::New { .. }
                            | varn_core::ast::ExprKind::Sequence { .. }
                            | varn_core::ast::ExprKind::Paren { .. }
                            | varn_core::ast::ExprKind::Await { .. }
                            | varn_core::ast::ExprKind::Spawn { .. }
                            | varn_core::ast::ExprKind::Yield { .. }
                            | varn_core::ast::ExprKind::Spread { .. }
                            | varn_core::ast::ExprKind::Pipeline { .. }
                            | varn_core::ast::ExprKind::Range { .. }
                            | varn_core::ast::ExprKind::NonNull { .. }
                            | varn_core::ast::ExprKind::Try { .. }
                            | varn_core::ast::ExprKind::As { .. }
                            | varn_core::ast::ExprKind::Satisfies { .. }
                            | varn_core::ast::ExprKind::Match { .. }
                            | varn_core::ast::ExprKind::Is { .. }
                            | varn_core::ast::ExprKind::With { .. }
                            | varn_core::ast::ExprKind::MetaAccess { .. } => {}
                        },
                        varn_core::ast::StmtKind::Block { .. }
                        | varn_core::ast::StmtKind::Empty
                        | varn_core::ast::StmtKind::Expr { .. }
                        | varn_core::ast::StmtKind::Decl(_)
                        | varn_core::ast::StmtKind::Error
                        | varn_core::ast::StmtKind::If { .. }
                        | varn_core::ast::StmtKind::While { .. }
                        | varn_core::ast::StmtKind::DoWhile { .. }
                        | varn_core::ast::StmtKind::For { .. }
                        | varn_core::ast::StmtKind::ForIn { .. }
                        | varn_core::ast::StmtKind::ForOf { .. }
                        | varn_core::ast::StmtKind::Switch { .. }
                        | varn_core::ast::StmtKind::Return { .. }
                        | varn_core::ast::StmtKind::Break { .. }
                        | varn_core::ast::StmtKind::Continue { .. }
                        | varn_core::ast::StmtKind::Throw { .. }
                        | varn_core::ast::StmtKind::Try { .. }
                        | varn_core::ast::StmtKind::Using { .. }
                        | varn_core::ast::StmtKind::Labeled { .. }
                        | varn_core::ast::StmtKind::Debugger => {
                            reason = Some("bodies that are not a single `return`")
                        }
                    }
                }
                varn_core::ast::StmtKind::Block { .. }
                | varn_core::ast::StmtKind::Empty
                | varn_core::ast::StmtKind::Expr { .. }
                | varn_core::ast::StmtKind::Decl(_)
                | varn_core::ast::StmtKind::Error
                | varn_core::ast::StmtKind::If { .. }
                | varn_core::ast::StmtKind::While { .. }
                | varn_core::ast::StmtKind::DoWhile { .. }
                | varn_core::ast::StmtKind::For { .. }
                | varn_core::ast::StmtKind::ForIn { .. }
                | varn_core::ast::StmtKind::ForOf { .. }
                | varn_core::ast::StmtKind::Switch { .. }
                | varn_core::ast::StmtKind::Return { .. }
                | varn_core::ast::StmtKind::Break { .. }
                | varn_core::ast::StmtKind::Continue { .. }
                | varn_core::ast::StmtKind::Throw { .. }
                | varn_core::ast::StmtKind::Try { .. }
                | varn_core::ast::StmtKind::Using { .. }
                | varn_core::ast::StmtKind::Labeled { .. }
                | varn_core::ast::StmtKind::Debugger => {
                    reason = Some("bodies that are not a single `return`")
                }
            }
        }
        if let Some(what) = reason {
            self.emit(
                varn_core::diagnostics::Diagnostic::error(
                    varn_core::diagnostics::ErrorCode::InvalidDecoratorSignature,
                    format!("`@inline` cannot apply to {what} (function '{name}')"),
                )
                .with_range(f.range),
            );
        }
    }
}
