use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use std::sync::Arc;
use varn_core::ast::ClassMember;
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_enum(&mut self, e: &varn_core::ast::EnumDecl, bind: &BindResult) {
        let saved_class = self
            .current_class
            .replace(Arc::from(bind.interner.resolve(e.id)));
        let saved_scope = self.current_scope;
        if let Some(enum_scope) = self.next_child_scope(bind) {
            self.current_scope = enum_scope;
        }

        for member in &e.body {
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
                ClassMember::Method {
                    type_params,
                    return_type,
                    body,
                    range,
                    ..
                } => {
                    if let Some(body_stmt) = *body {
                        let saved_method_scope = self.current_scope;
                        if let Some(m_scope) = self.next_child_scope(bind) {
                            self.current_scope = m_scope;
                            self.record_scope(range.start.offset);
                        }

                        let saved_expected = self.expected_return_type.take();
                        self.expected_return_type = return_type
                            .as_ref()
                            .map(|rt| self.resolve_type_node_cached(rt, bind));

                        for tp in type_params {
                            self.active_type_params
                                .insert(Arc::from(bind.interner.resolve(tp.name)));
                        }

                        let saved_in_function = self.in_function;
                        self.in_function = true;

                        self.check_stmt(body_stmt, bind);

                        self.in_function = saved_in_function;

                        for tp in type_params {
                            self.active_type_params
                                .remove(bind.interner.resolve(tp.name));
                        }

                        self.expected_return_type = saved_expected;
                        self.current_scope = saved_method_scope;
                    }
                }
                ClassMember::Getter {
                    return_type,
                    body,
                    range,
                    ..
                } => {
                    if let Some(body_stmt) = *body {
                        let saved_getter_scope = self.current_scope;
                        if let Some(g_scope) = self.next_child_scope(bind) {
                            self.current_scope = g_scope;
                            self.record_scope(range.start.offset);
                        }

                        let saved_expected = self.expected_return_type.take();
                        self.expected_return_type = return_type
                            .as_ref()
                            .map(|rt| self.resolve_type_node_cached(rt, bind));

                        let saved_in_function = self.in_function;
                        self.in_function = true;

                        self.check_stmt(body_stmt, bind);

                        self.in_function = saved_in_function;

                        self.expected_return_type = saved_expected;
                        self.current_scope = saved_getter_scope;
                    }
                }
                ClassMember::Setter {
                    key,
                    param,
                    body,
                    range,
                    ..
                } => {
                    if let Some(body_stmt) = *body {
                        let saved_setter_scope = self.current_scope;
                        if let Some(s_scope) = self.next_child_scope(bind) {
                            self.current_scope = s_scope;
                            self.record_scope(range.start.offset);
                        }

                        let mut param_ty = param
                            .type_ann
                            .as_ref()
                            .map(|node| self.resolve_type_node_cached(node, bind))
                            .unwrap_or(Type::Dynamic);

                        if param_ty.is_dynamic() {
                            if let Some(ref class_name) = self.current_class {
                                if let Some(members) =
                                    bind.get_class_entry(class_name).map(|e| e.members.clone())
                                {
                                    let key_str = bind.interner.resolve(*key);
                                    if let Some(m) =
                                        members.iter().find(|m| m.name.as_ref() == key_str)
                                    {
                                        param_ty = m.ty;
                                    }
                                }
                            }
                        }

                        self.check_pattern(&param.pattern, &param_ty, bind);

                        let saved_in_function = self.in_function;
                        self.in_function = true;

                        self.check_stmt(body_stmt, bind);

                        self.in_function = saved_in_function;

                        self.current_scope = saved_setter_scope;
                    }
                }
                ClassMember::Constructor { body, .. } => {
                    let body = *body;
                    let saved_in_function = self.in_function;
                    self.in_function = true;
                    let saved_scope = self.current_scope;
                    if let Some(ctor_scope) = self.next_child_scope(bind) {
                        self.current_scope = ctor_scope;
                        self.record_scope(self.ast_arena.stmt(body).range.start.offset);
                    }
                    self.check_stmt(body, bind);
                    self.current_scope = saved_scope;
                    self.in_function = saved_in_function;
                }
                ClassMember::StaticBlock { body, range } => {
                    let body = *body;
                    let saved_block_scope = self.current_scope;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope(range.start.offset);
                    }
                    self.check_stmt(body, bind);
                    self.current_scope = saved_block_scope;
                }
                _ => {}
            }
        }

        self.current_scope = saved_scope;
        self.current_class = saved_class;
    }
}
