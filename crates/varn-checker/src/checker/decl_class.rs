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
                    if super::decorator_receiver::method_uses_receiver(
                        self.ast_arena,
                        params,
                        *body,
                    ) {
                        let key_str = bind.interner.resolve(*key);
                        self.emit(
                            Diagnostic::error(
                                ErrorCode::InvalidDecoratorTarget,
                                format!("decorated method '{key_str}' cannot use 'this' or 'super': the wrapper loses the receiver"),
                            )
                            .with_range(*range),
                        );
                    }
                }
                ClassMember::Property {
                    key,
                    decorators,
                    range,
                    ..
                } if !decorators.is_empty() => {
                    let key_str = bind.interner.resolve(*key);
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidDecoratorTarget,
                            format!("decorators are not supported on property '{key_str}'"),
                        )
                        .with_range(*range),
                    );
                }
                _ => {}
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
                _ => {}
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
                    ..
                } => {
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
                ClassMember::Constructor { body, .. } => {
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
                    self.in_function_body(false, |c| c.check_stmt(body, bind));
                    self.current_scope = saved_scope;
                }
                ClassMember::Method {
                    return_type,
                    body: Some(body),
                    modifiers,
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

                    self.in_function_body(modifiers.is_async, |c| c.check_stmt(body, bind));

                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                ClassMember::Getter {
                    return_type,
                    body: Some(body),
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

                    self.in_function_body(false, |c| c.check_stmt(body, bind));

                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                ClassMember::Setter {
                    body: Some(body), ..
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

                    self.in_function_body(false, |c| c.check_stmt(body, bind));

                    self.current_scope = saved_scope;
                }
                _ => {}
            }
        }
    }
}
