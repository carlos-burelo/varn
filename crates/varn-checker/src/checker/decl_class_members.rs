use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::ClassMember;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_class_members(
        &mut self,
        rec: &mut Recorder,
        c: &varn_core::ast::ClassDecl,
        bind: &BindResult,
    ) {
        for member in &c.body {
            match member {
                ClassMember::Property {
                    key,
                    type_ann,
                    init,
                    range,
                    decorators,
                    ..
                } => {
                    if !decorators.is_empty() {
                        let key_str = bind.interner.resolve(*key);
                        self.check_decorator_signatures(
                            rec,
                            decorators,
                            super::decorator_signature::DecoratorTarget::Property,
                            key_str,
                            bind,
                        );
                    }
                    if let Some(init_expr) = *init {
                        if let Some(ann) = type_ann {
                            let prop_ty = self.resolve_type_node_cached(ann, bind);
                            let key_str = bind.interner.resolve(*key);
                            self.with_expected(Some(prop_ty), |checker| {
                                checker.check_expr(rec, init_expr, bind);
                                let init_ty = checker.infer_type(rec, init_expr, bind);
                                if !checker.value_assignable_to(&prop_ty, &init_ty, Some(init_expr), Some(bind)) {
                                    let prop_ty_s = prop_ty.display(&checker.ty_table, &bind.interner);
                                    let init_ty_s = init_ty.display(&checker.ty_table, &bind.interner);
                                    checker.emit(
                                        Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                            "type mismatch: property '{}' is declared as '{}' but initialised with '{}'",
                                            key_str, prop_ty_s, init_ty_s
                                        ))
                                        .with_range(*range),
                                    );
                                }
                            });
                        }
                    }
                }
                ClassMember::Constructor {
                    body, decorators, ..
                } => {
                    let body = *body;
                    let saved_scope = self.current_scope;
                    let body_range = self.ast_arena.stmt(body).range;
                    if let Some(ctor_scope) = self.next_child_scope(bind) {
                        self.current_scope = ctor_scope;
                        self.record_scope_span(
                            rec,
                            body_range.start.offset,
                            body_range.end.offset,
                            ctor_scope,
                        );
                    }
                    let saved_caps = self
                        .enclosing_caps
                        .replace(super::decorator_purity::caps_of(
                            decorators,
                            self.ast_arena,
                            bind,
                        ));
                    self.in_function_body(false, |c| c.check_stmt(rec, body, bind));
                    self.enclosing_caps = saved_caps;
                    self.current_scope = saved_scope;
                }
                ClassMember::Method {
                    return_type,
                    body: Some(body),
                    modifiers,
                    decorators,
                    ..
                } => {
                    let body = *body;
                    let saved_expected = self.expected_return_type.take();
                    self.expected_return_type = return_type.as_ref().map(|rt| {
                        let ty = self.resolve_type_node_cached(rt, bind);
                        if modifiers.is_async {
                            varn_sem::types::awaited(&ty, &self.ty_table)
                        } else {
                            ty
                        }
                    });

                    let saved_scope = self.current_scope;
                    let body_range = self.ast_arena.stmt(body).range;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope_span(
                            rec,
                            body_range.start.offset,
                            body_range.end.offset,
                            m_scope,
                        );
                    }

                    let saved_caps = self
                        .enclosing_caps
                        .replace(super::decorator_purity::caps_of(
                            decorators,
                            self.ast_arena,
                            bind,
                        ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_purity::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(modifiers.is_async, |c| c.check_stmt(rec, body, bind));

                    self.pure_scope = saved_pure;
                    self.enclosing_caps = saved_caps;
                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                ClassMember::Getter {
                    return_type,
                    body: Some(body),
                    decorators,
                    ..
                } => {
                    let body = *body;
                    let saved_expected = self.expected_return_type.take();
                    self.expected_return_type = return_type
                        .as_ref()
                        .map(|rt| self.resolve_type_node_cached(rt, bind));

                    let saved_scope = self.current_scope;
                    let body_range = self.ast_arena.stmt(body).range;
                    if let Some(g_scope) = self.next_child_scope(bind) {
                        self.current_scope = g_scope;
                        self.record_scope_span(
                            rec,
                            body_range.start.offset,
                            body_range.end.offset,
                            g_scope,
                        );
                    }
                    let saved_caps = self
                        .enclosing_caps
                        .replace(super::decorator_purity::caps_of(
                            decorators,
                            self.ast_arena,
                            bind,
                        ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_purity::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(false, |c| c.check_stmt(rec, body, bind));

                    self.pure_scope = saved_pure;
                    self.enclosing_caps = saved_caps;
                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                ClassMember::Setter {
                    body: Some(body),
                    decorators,
                    ..
                } => {
                    let body = *body;
                    let saved_scope = self.current_scope;
                    let body_range = self.ast_arena.stmt(body).range;
                    if let Some(s_scope) = self.next_child_scope(bind) {
                        self.current_scope = s_scope;
                        self.record_scope_span(
                            rec,
                            body_range.start.offset,
                            body_range.end.offset,
                            s_scope,
                        );
                    }
                    let saved_caps = self
                        .enclosing_caps
                        .replace(super::decorator_purity::caps_of(
                            decorators,
                            self.ast_arena,
                            bind,
                        ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_purity::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(false, |c| c.check_stmt(rec, body, bind));

                    self.pure_scope = saved_pure;
                    self.enclosing_caps = saved_caps;
                    self.current_scope = saved_scope;
                }
                ClassMember::Destructor { .. }
                | ClassMember::Method { .. }
                | ClassMember::Getter { .. }
                | ClassMember::Setter { .. }
                | ClassMember::StaticBlock { .. } => {}
            }
        }
    }
}
