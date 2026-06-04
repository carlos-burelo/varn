use std::rc::Rc;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::{ClassMember, Decl, ExprKind, ExtensionMember};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl Checker {
    pub(crate) fn check_decls(&mut self, decls: &[Decl], bind: &BindResult) {
        for decl in decls {
            self.check_decl(decl, bind);
        }
    }

    pub(crate) fn check_decl(&mut self, decl: &Decl, bind: &BindResult) {
        match decl {
            Decl::Variable(v) => {
                for d in &v.declarators {
                    let ann = d.type_ann.as_ref().or(match &d.id {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

                    if let Some(init_expr) = &d.init {
                        self.with_expected(ann_ty_opt.clone(), |c| c.check_expr(init_expr, bind));

                        if let Some(ann_ty) = &ann_ty_opt {
                            let init_ty = self.infer_type(init_expr, bind);
                            let is_empty_array = init_ty.is_dynamic()
                                && matches!(&init_expr.kind, ExprKind::Array { elements } if elements.is_empty());
                            if !is_empty_array
                                && !self.types_compatible_cached(ann_ty, &init_ty, Some(bind))
                            {
                                self.emit(
                                    Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                        "type mismatch: declared as '{ann_ty}' but initialised with '{init_ty}'"
                                    ))
                                    .with_range(*decl.range()),
                                );
                            }
                            self.check_pattern(&d.id, ann_ty, bind);
                        } else {
                            let init_ty = self.infer_type(init_expr, bind);
                            let final_ty = if v.kind == varn_core::ast::VarKind::Let {
                                crate::binder::widen_literal(init_ty)
                            } else {
                                init_ty
                            };
                            self.check_pattern(&d.id, &final_ty, bind);
                        }
                    }
                }
            }

            Decl::Function(f) => {
                let saved_expected = self.expected_return_type.take();
                self.expected_return_type = f
                    .return_type
                    .as_ref()
                    .map(|rt| self.resolve_type_node_cached(rt, bind));

                let saved_scope = self.current_scope;
                let next_scope = self.next_child_scope(bind);
                if let Some(fn_scope) = next_scope {
                    self.current_scope = fn_scope;
                    self.record_scope(f.body.range.start.offset);
                }
                let is_gen = f.modifiers.is_generator;
                let old_yields = if is_gen {
                    self.yielded_types.replace(Vec::new())
                } else {
                    None
                };

                self.check_stmt(&f.body, bind);

                if is_gen {
                    let yields = self.yielded_types.take().unwrap_or_default();
                    if f.return_type.is_none() {
                        let inferred_yield = if yields.is_empty() { Type::Void } else { Type::union(yields) };
                        let scope = bind.scopes.get(saved_scope);
                        if let Some(sym_id) = scope.resolve(&f.id, &bind.scopes) {
                            if let Some(mut fn_ty) = self.symbol_types.get(&sym_id).cloned().or_else(|| bind.arena.get(sym_id).ty.clone()) {
                                if let TypeKind::Fn(ref mut ft) = fn_ty.0 {
                                    ft.return_type = Box::new(Type::generic("Generator", vec![inferred_yield]));
                                }
                                self.symbol_types.insert(sym_id, fn_ty.clone());
                                self.record_type_with_symbol(f.id_offset, fn_ty, sym_id);
                            }
                        }
                    }
                    self.yielded_types = old_yields;
                }

                self.current_scope = saved_scope;
                self.expected_return_type = saved_expected;
            }

            Decl::Class(c) => {
                let name = c.id.clone().unwrap_or_else(|| Rc::from("<anon>"));
                let saved_class = self.current_class.replace(name);
                let saved_scope = self.current_scope;
                if let Some(cls_scope) = self.next_child_scope(bind) {
                    self.current_scope = cls_scope;
                }

                for member in &c.body {
                    match member {
                        ClassMember::Property {
                            key,
                            type_ann,
                            init,
                            range,
                            ..
                        } => {
                            if let Some(init_expr) = init {
                                if let Some(ann) = type_ann {
                                    let prop_ty = self.resolve_type_node_cached(ann, bind);
                                    self.with_expected(Some(prop_ty.clone()), |checker| {
                                        checker.check_expr(init_expr, bind);
                                        let init_ty = checker.infer_type(init_expr, bind);
                                        if !checker.types_compatible_cached(&prop_ty, &init_ty, Some(bind)) {
                                            checker.emit(
                                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                                    "type mismatch: property '{}' is declared as '{}' but initialised with '{}'",
                                                    key, prop_ty, init_ty
                                                ))
                                                .with_range(*range),
                                            );
                                        }
                                    });
                                }
                            }
                        }
                        ClassMember::Constructor { body, .. } => {
                            let saved_scope = self.current_scope;
                            if let Some(ctor_scope) = self.next_child_scope(bind) {
                                self.current_scope = ctor_scope;
                                self.record_scope(body.range.start.offset);
                            }
                            self.check_stmt(body, bind);
                            self.current_scope = saved_scope;
                        }
                        ClassMember::Method {
                            return_type,
                            body: Some(body),
                            ..
                        } => {
                            let saved_expected = self.expected_return_type.take();
                            self.expected_return_type = return_type
                                .as_ref()
                                .map(|rt| self.resolve_type_node_cached(rt, bind));

                            let saved_scope = self.current_scope;
                            if let Some(m_scope) = self.next_child_scope(bind) {
                                self.current_scope = m_scope;
                                self.record_scope(body.range.start.offset);
                            }

                            self.check_stmt(body, bind);

                            self.current_scope = saved_scope;
                            self.expected_return_type = saved_expected;
                        }
                        ClassMember::Getter {
                            return_type,
                            body: Some(body),
                            ..
                        } => {
                            let saved_expected = self.expected_return_type.take();
                            self.expected_return_type = return_type
                                .as_ref()
                                .map(|rt| self.resolve_type_node_cached(rt, bind));

                            let saved_scope = self.current_scope;
                            if let Some(g_scope) = self.next_child_scope(bind) {
                                self.current_scope = g_scope;
                                self.record_scope(body.range.start.offset);
                            }

                            self.check_stmt(body, bind);

                            self.current_scope = saved_scope;
                            self.expected_return_type = saved_expected;
                        }
                        ClassMember::Setter {
                            body: Some(body), ..
                        } => {
                            let saved_scope = self.current_scope;
                            if let Some(s_scope) = self.next_child_scope(bind) {
                                self.current_scope = s_scope;
                                self.record_scope(body.range.start.offset);
                            }

                            self.check_stmt(body, bind);

                            self.current_scope = saved_scope;
                        }
                        _ => {}
                    }
                }

                self.current_scope = saved_scope;
                self.current_class = saved_class;
            }

            Decl::Enum(e) => {
                let saved_class = self.current_class.replace(e.id.clone());
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
                            if let Some(init_expr) = init {
                                if let Some(ann) = type_ann {
                                    let prop_ty = self.resolve_type_node_cached(ann, bind);
                                    self.with_expected(Some(prop_ty.clone()), |checker| {
                                        checker.check_expr(init_expr, bind);
                                        let init_ty = checker.infer_type(init_expr, bind);
                                        if !checker.types_compatible_cached(&prop_ty, &init_ty, Some(bind)) {
                                            checker.emit(
                                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                                    "type mismatch: property '{}' is declared as '{}' but initialised with '{}'",
                                                    key, prop_ty, init_ty
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
                            if let Some(body_stmt) = body {
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
                                    self.active_type_params.insert(Rc::from(tp.name.as_str()));
                                }

                                self.check_stmt(body_stmt, bind);

                                for tp in type_params {
                                    self.active_type_params.remove(tp.name.as_str());
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
                            if let Some(body_stmt) = body {
                                let saved_getter_scope = self.current_scope;
                                if let Some(g_scope) = self.next_child_scope(bind) {
                                    self.current_scope = g_scope;
                                    self.record_scope(range.start.offset);
                                }

                                let saved_expected = self.expected_return_type.take();
                                self.expected_return_type = return_type
                                    .as_ref()
                                    .map(|rt| self.resolve_type_node_cached(rt, bind));

                                self.check_stmt(body_stmt, bind);

                                self.expected_return_type = saved_expected;
                                self.current_scope = saved_getter_scope;
                            }
                        }
                        ClassMember::Setter {
                            param, body, range, ..
                        } => {
                            if let Some(body_stmt) = body {
                                let saved_setter_scope = self.current_scope;
                                if let Some(s_scope) = self.next_child_scope(bind) {
                                    self.current_scope = s_scope;
                                    self.record_scope(range.start.offset);
                                }

                                let param_ty = param
                                    .type_ann
                                    .as_ref()
                                    .map(|node| self.resolve_type_node_cached(node, bind))
                                    .unwrap_or(Type::Dynamic);

                                self.check_pattern(&param.pattern, &param_ty, bind);
                                self.check_stmt(body_stmt, bind);

                                self.current_scope = saved_setter_scope;
                            }
                        }
                        ClassMember::Constructor { body, .. } => {
                            let saved_scope = self.current_scope;
                            if let Some(ctor_scope) = self.next_child_scope(bind) {
                                self.current_scope = ctor_scope;
                                self.record_scope(body.range.start.offset);
                            }
                            self.check_stmt(body, bind);
                            self.current_scope = saved_scope;
                        }
                        ClassMember::StaticBlock { body, range } => {
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

            Decl::Interface(_) => {
                self.next_child_scope(bind);
            }

            Decl::Extension(ext) => {
                let ext_self_ty = self.resolve_type_node_cached(&ext.target, bind);
                let ext_class_name = match &ext_self_ty.0 {
                    TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(n.clone()),
                    TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                        Some(varn_core::IntrinsicType::Str.as_str().into())
                    }
                    TypeKind::Intrinsic(varn_core::TypeTag::Int) => {
                        Some(varn_core::IntrinsicType::Int.as_str().into())
                    }
                    TypeKind::Intrinsic(varn_core::TypeTag::Float) => {
                        Some(varn_core::IntrinsicType::Float.as_str().into())
                    }
                    TypeKind::Intrinsic(varn_core::TypeTag::Bool) => {
                        Some(varn_core::IntrinsicType::Bool.as_str().into())
                    }
                    _ => None,
                };
                let saved_class = self
                    .current_class
                    .replace(ext_class_name.unwrap_or_else(|| Rc::from("_")));

                for member in &ext.members {
                    let saved_expected = self.expected_return_type.take();
                    match member {
                        ExtensionMember::Method(method) => {
                            self.expected_return_type = method
                                .return_type
                                .as_ref()
                                .map(|rt| self.resolve_type_node_cached(rt, bind));
                            let saved_scope = self.current_scope;
                            if let Some(m_scope) = self.next_child_scope(bind) {
                                self.current_scope = m_scope;
                                self.record_scope(method.body.range.start.offset);
                            }
                            self.check_stmt(&method.body, bind);
                            self.current_scope = saved_scope;
                        }
                        ExtensionMember::Getter {
                            return_type, body, ..
                        } => {
                            self.expected_return_type = return_type
                                .as_ref()
                                .map(|rt| self.resolve_type_node_cached(rt, bind));
                            let saved_scope = self.current_scope;
                            if let Some(m_scope) = self.next_child_scope(bind) {
                                self.current_scope = m_scope;
                                self.record_scope(body.range.start.offset);
                            }
                            self.check_stmt(body, bind);
                            self.current_scope = saved_scope;
                        }
                        ExtensionMember::Setter { body, .. } => {
                            self.expected_return_type = Some(Type::Void);
                            let saved_scope = self.current_scope;
                            if let Some(m_scope) = self.next_child_scope(bind) {
                                self.current_scope = m_scope;
                                self.record_scope(body.range.start.offset);
                            }
                            self.check_stmt(body, bind);
                            self.current_scope = saved_scope;
                        }
                    }
                    self.expected_return_type = saved_expected;
                }

                self.current_class = saved_class;
            }

            Decl::Namespace(ns) => {
                let saved_scope = self.current_scope;
                if let Some(ns_scope) = self.next_child_scope(bind) {
                    self.current_scope = ns_scope;
                    self.record_scope(ns.range.start.offset);
                }
                self.check_decls(&ns.body, bind);
                self.current_scope = saved_scope;
            }

            Decl::Export(e) => {
                self.check_export(e, bind);
            }

            _ => {}
        }
    }

    pub(crate) fn check_export(&mut self, e: &varn_core::ast::ExportDecl, bind: &BindResult) {
        match e {
            varn_core::ast::ExportDecl::Decl { declaration, .. } => {
                self.check_decl(declaration, bind);
            }
            varn_core::ast::ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                varn_core::ast::ExportDefaultDecl::Function(f) => {
                    self.check_decl(&varn_core::ast::Decl::Function(f.clone()), bind);
                }
                varn_core::ast::ExportDefaultDecl::Class(c) => {
                    self.check_decl(&varn_core::ast::Decl::Class(c.clone()), bind);
                }
                varn_core::ast::ExportDefaultDecl::Expr(expr) => {
                    self.check_expr(expr, bind);
                }
            },
            _ => {}
        }
    }
}
