use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use std::sync::Arc;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(super) fn check_extension(
        &mut self,
        ext: &varn_core::ast::ExtensionDecl,
        bind: &BindResult,
    ) {
        let ext_self_ty = self.resolve_type_node_cached(&ext.target, bind);
        let ext_class_name = match self.ty_table.get(ext_self_ty.0) {
            TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
                Some(Arc::from(bind.interner.resolve(n)))
            }

            TypeKind::Primitive(p) if p != varn_core::LangPrimitive::Dynamic => {
                Some(p.name().into())
            }
            _ => None,
        };
        let saved_class = self
            .current_class
            .replace(ext_class_name.unwrap_or_else(|| Arc::from("_")));

        for member in &ext.members {
            let saved_expected = self.expected_return_type.take();
            match member {
                varn_core::ast::ExtensionMember::Method(method) => {
                    self.expected_return_type = method.return_type.as_ref().map(|rt| {
                        let ty = self.resolve_type_node_cached(rt, bind);
                        if method.modifiers.is_async {
                            crate::types::awaited(&ty, &self.ty_table, &bind.interner)
                        } else {
                            ty
                        }
                    });
                    let saved_in_function = self.in_function;
                    self.in_function = true;
                    let saved_scope = self.current_scope;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope(self.ast_arena.stmt(method.body).range.start.offset);
                    }
                    self.check_stmt(method.body, bind);
                    self.current_scope = saved_scope;
                    self.in_function = saved_in_function;
                }
                varn_core::ast::ExtensionMember::Getter {
                    return_type, body, ..
                } => {
                    let body = *body;
                    self.expected_return_type = return_type
                        .as_ref()
                        .map(|rt| self.resolve_type_node_cached(rt, bind));
                    let saved_in_function = self.in_function;
                    self.in_function = true;
                    let saved_scope = self.current_scope;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope(self.ast_arena.stmt(body).range.start.offset);
                    }
                    self.check_stmt(body, bind);
                    self.current_scope = saved_scope;
                    self.in_function = saved_in_function;
                }
                varn_core::ast::ExtensionMember::Setter { body, .. } => {
                    let body = *body;
                    self.expected_return_type = Some(Type::Void);
                    let saved_in_function = self.in_function;
                    self.in_function = true;
                    let saved_scope = self.current_scope;
                    if let Some(m_scope) = self.next_child_scope(bind) {
                        self.current_scope = m_scope;
                        self.record_scope(self.ast_arena.stmt(body).range.start.offset);
                    }
                    self.check_stmt(body, bind);
                    self.current_scope = saved_scope;
                    self.in_function = saved_in_function;
                }
            }
            self.expected_return_type = saved_expected;
        }

        self.current_class = saved_class;
    }

    pub(super) fn check_export(&mut self, e: &varn_core::ast::ExportDecl, bind: &BindResult) {
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
                    self.check_expr(*expr, bind);
                }
            },
            _ => {}
        }
    }
}
