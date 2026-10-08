use super::Checker;
use crate::binder::BindResult;
use crate::types::ClassMemberKind;
use std::sync::Arc;
use varn_core::ast::ClassMember;
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_class(&mut self, c: &varn_core::ast::ClassDecl, bind: &BindResult) {
        if c.modifiers.is_abstract {
            if let Some(id) = &c.id {
                self.abstract_classes
                    .insert(Arc::from(bind.interner.resolve(*id)));
            }
        }
        let name =
            c.id.map(|id| Arc::from(bind.interner.resolve(id)))
                .unwrap_or_else(|| Arc::from("<anon>"));
        let saved_class = self.current_class.replace(name);
        let saved_scope = self.current_scope;
        if let Some(cls_scope) = self.next_child_scope(bind) {
            self.current_scope = cls_scope;
        }

        let mut superclass_members = Vec::new();
        let mut parent =
            c.id.as_ref()
                .and_then(|cls_id| bind.class_parents.get(bind.interner.resolve(*cls_id)));
        while let Some(p) = parent {
            if let Some(m) = bind.get_class_entry(&p.name).map(|e| e.members.clone()) {
                superclass_members.extend(m);
            }
            parent = bind.class_parents.get(&p.name);
        }

        self.check_class_decorators(c, bind);
        self.check_class_overrides(c, &superclass_members, bind);
        self.check_class_members(c, bind);

        self.current_scope = saved_scope;
        self.current_class = saved_class;
    }

    fn check_class_decorators(&mut self, c: &varn_core::ast::ClassDecl, bind: &BindResult) {
        if !c.decorators.is_empty() {
            let name =
                c.id.map(|id| bind.interner.resolve(id))
                    .unwrap_or("<anonymous>");
            self.check_decorator_signatures(
                &c.decorators,
                super::decorator_signature::DecoratorTarget::Class,
                name,
                bind,
            );
        }
        for member in &c.body {
            match member {
                ClassMember::Method {
                    key,
                    body: Some(body),
                    params,
                    modifiers,
                    range,
                    ..
                } if modifiers.is_static
                    && super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) =>
                {
                    let key_str = bind.interner.resolve(*key);
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::ThisOutsideInstance,
                            format!("static method '{key_str}' cannot use 'this' or 'super': no receiver"),
                        )
                        .with_range(*range),
                    );
                }
                ClassMember::Method {
                    key,
                    decorators,
                    body: Some(body),
                    params,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        decorators,
                        super::decorator_signature::DecoratorTarget::Method,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated method '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor {
                    decorators,
                    body,
                    params,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    self.check_decorator_signatures(
                        decorators,
                        super::decorator_signature::DecoratorTarget::Constructor,
                        "constructor",
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                "decorated constructor cannot use 'this' or 'super': the wrapper loses the receiver".to_owned(),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Getter {
                    key,
                    decorators,
                    body: Some(body),
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        decorators,
                        super::decorator_signature::DecoratorTarget::Getter,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(self.ast_arena, &[], *body) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated getter '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Setter {
                    key,
                    decorators,
                    body: Some(body),
                    param,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.check_decorator_signatures(
                        decorators,
                        super::decorator_signature::DecoratorTarget::Setter,
                        key_str,
                        bind,
                    );
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        std::slice::from_ref(param),
                        *body,
                    ) {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated setter '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::Method { .. }
                | ClassMember::Property { .. }
                | ClassMember::Getter { .. }
                | ClassMember::Setter { .. }
                | ClassMember::StaticBlock { .. } => {}
            }
        }
    }

    fn check_class_overrides(
        &mut self,
        c: &varn_core::ast::ClassDecl,
        superclass_members: &[crate::types::ClassMemberInfo],
        bind: &BindResult,
    ) {
        for member in &c.body {
            match member {
                ClassMember::Method {
                    key,
                    modifiers,
                    range,
                    ..
                }
                | ClassMember::Getter {
                    key,
                    modifiers,
                    range,
                    ..
                }
                | ClassMember::Setter {
                    key,
                    modifiers,
                    range,
                    ..
                } => {
                    let is_override = modifiers.is_override;
                    let key_str = bind.interner.resolve(*key);
                    let exists_in_superclass = superclass_members.iter().any(|m| {
                        m.name.as_ref() == key_str && m.kind != ClassMemberKind::Constructor
                    });
                    if exists_in_superclass {
                        if !is_override {
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::MissingOverride,
                                    format!("member '{}' overrides a member in the superclass but is missing the 'override' modifier", key_str),
                                )
                                .with_range(*range),
                            );
                        }
                    } else if is_override {
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::SpuriousOverride,
                                format!("member '{}' is marked as override but does not override any member in the superclass", key_str),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Constructor { .. }
                | ClassMember::Destructor { .. }
                | ClassMember::Property { .. }
                | ClassMember::StaticBlock { .. } => {}
            }
        }
    }

    pub(super) fn check_class_members(&mut self, c: &varn_core::ast::ClassDecl, bind: &BindResult) {
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
                                checker.check_expr(init_expr, bind);
                                let init_ty = checker.infer_type(init_expr, bind);
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
                            body_range.start.offset,
                            body_range.end.offset,
                            ctor_scope,
                        );
                    }
                    let saved_caps =
                        self.enclosing_caps
                            .replace(super::decorator_signature::caps_of(
                                decorators,
                                self.ast_arena,
                                bind,
                            ));
                    self.in_function_body(false, |c| c.check_stmt(body, bind));
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
                            crate::types::awaited(&ty, &self.ty_table, &bind.interner)
                        } else {
                            ty
                        }
                    });

                    let saved_scope = self.current_scope;
                    let body_range = self.ast_arena.stmt(body).range;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope_span(
                            body_range.start.offset,
                            body_range.end.offset,
                            m_scope,
                        );
                    }

                    let saved_caps =
                        self.enclosing_caps
                            .replace(super::decorator_signature::caps_of(
                                decorators,
                                self.ast_arena,
                                bind,
                            ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_signature::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(modifiers.is_async, |c| c.check_stmt(body, bind));

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
                            body_range.start.offset,
                            body_range.end.offset,
                            g_scope,
                        );
                    }
                    let saved_caps =
                        self.enclosing_caps
                            .replace(super::decorator_signature::caps_of(
                                decorators,
                                self.ast_arena,
                                bind,
                            ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_signature::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(false, |c| c.check_stmt(body, bind));

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
                            body_range.start.offset,
                            body_range.end.offset,
                            s_scope,
                        );
                    }
                    let saved_caps =
                        self.enclosing_caps
                            .replace(super::decorator_signature::caps_of(
                                decorators,
                                self.ast_arena,
                                bind,
                            ));
                    let saved_pure = self.pure_scope.take();
                    if super::decorator_signature::is_pure_fn(decorators, self.ast_arena, bind) {
                        self.pure_scope = Some(self.current_scope);
                    }

                    self.in_function_body(false, |c| c.check_stmt(body, bind));

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
