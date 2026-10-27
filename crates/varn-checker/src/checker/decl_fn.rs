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
        self.check_signature_type_names(&f.type_params, &f.params, f.return_type.as_ref(), bind);
        let saved_expected = self.expected_return_type.take();
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
        self.expected_return_type = f.return_type.as_ref().map(|rt| {
            let ty = self.resolve_type_node_cached(rt, bind);
            if f.modifiers.is_async {
                varn_sem::types::awaited(&ty, &self.ty_table)
            } else {
                ty
            }
        });

        let is_gen = f.modifiers.is_generator;
        let declared_yield = f.return_type.as_ref().filter(|_| is_gen).map(|ann| {
            let declared = self.resolve_type_node_cached(ann, bind);
            varn_sem::types::yielded_by(declared, &self.ty_table)
        });
        let old_declared_yield = std::mem::replace(&mut self.declared_yield, declared_yield);
        let old_yields = if is_gen {
            self.yielded_types.replace(Vec::new())
        } else {
            None
        };

        let saved_caps = self
            .enclosing_caps
            .replace(super::decorator_purity::caps_of(
                &f.decorators,
                self.ast_arena,
                bind,
            ));
        let saved_pure = self.pure_scope.take();
        if super::decorator_purity::is_pure_fn(&f.decorators, self.ast_arena, bind) {
            self.pure_scope = next_scope.or(Some(self.current_scope));
        }

        self.check_param_defaults(rec, &f.params, bind);
        if !f.modifiers.is_declare && !f.modifiers.is_native {
            let effects = self.closure_assigned_symbols(
                super::stmts::closure_effects::ClosureBodyRef::Stmt(f.body),
                bind,
            );
            if let Some(id) = bind
                .scopes
                .get(self.current_scope)
                .resolve(f.id, &bind.scopes)
            {
                let entry = self.closure_effects.entry(id).or_default();
                entry.vars.extend(effects.vars.iter().copied());
                entry.vars.sort();
                entry.vars.dedup();
                entry.members.extend(effects.members.iter().cloned());
                entry.members.sort();
                entry.members.dedup();
            }
        }
        self.in_function_body(f.modifiers.is_async, |c| c.check_stmt(rec, f.body, bind));
        let has_body = !f.modifiers.is_declare && !f.modifiers.is_native;
        if f.return_type.is_some() && !is_gen && has_body {
            self.require_return(rec, f.body, bind);
        }

        self.pure_scope = saved_pure;
        self.enclosing_caps = saved_caps;

        let inferred = if is_gen {
            let yields = self.yielded_types.take().unwrap_or_default();
            self.yielded_types = old_yields;
            f.return_type.is_none().then(|| {
                let yielded = if yields.is_empty() {
                    Type::Void
                } else {
                    Type::union(yields, Arc::make_mut(&mut self.ty_table))
                };
                varn_sem::types::generator_of(
                    yielded,
                    f.modifiers.is_async,
                    Arc::make_mut(&mut self.ty_table),
                )
            })
        } else if f.return_type.is_none() && has_body {
            let returned = crate::checker_expressions::infer::arrow_body_return_type(
                varn_core::ast::ArrowBody::Block(f.body),
                self,
                rec,
                bind,
            );
            Some(varn_sem::types::async_fn_return(
                returned,
                f.modifiers.is_async,
                Arc::make_mut(&mut self.ty_table),
            ))
        } else {
            None
        };
        if let Some(returned) = inferred {
            let scope = bind.scopes.get(saved_scope);
            if let Some(sym_id) = scope.resolve(f.id, &bind.scopes)
                && let Some(fn_ty) = rec
                    .symbol_types
                    .get(&sym_id)
                    .copied()
                    .or_else(|| bind.arena.get(sym_id).ty)
                && let varn_core::TypeKind::Fn(fid) = self.ty_table.get(fn_ty.0)
            {
                let mut ft = self.ty_table.get_function(fid).clone();
                ft.return_type = returned.0;
                let new_fn_ty = Type::fn_(ft, Arc::make_mut(&mut self.ty_table));
                self.record_type_with_symbol(rec, f.id_offset, new_fn_ty, sym_id);
            }
        }

        self.declared_yield = old_declared_yield;
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
            if self.has_builtin(
                &f.decorators,
                bind,
                varn_core::ast::decorator_kind::BuiltinDecoratorKind::Inline,
            ) {
                self.check_inline_shape(f, name);
            }
        }
    }
}
