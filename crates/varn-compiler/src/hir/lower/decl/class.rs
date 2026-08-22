//! Class declarations: members, methods, and the declarations that can appear
//! inline in a class body.

use std::rc::Rc;

use varn_core::ast::decl::{ClassDecl, ClassMember};
use varn_core::ast::{Decl, Decorator, Expr, Modifiers, Param, Stmt, Visibility};

use super::super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::hir::lower) fn lower_class(
        &mut self,
        decl: &ClassDecl,
        scope: &mut Scope,
    ) -> R<HirClass> {
        let name = decl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
        let mut class_decorators = Vec::new();
        for dec in &decl.decorators {
            class_decorators.push(self.lower_expr(&dec.expression, scope)?);
        }
        let super_class = match &decl.super_class {
            Some(e) => Some(self.lower_expr(e, scope)?),
            None => None,
        };

        let mut fields: Vec<Rc<str>> = Vec::new();
        let mut field_inits: Vec<(Rc<str>, &Expr)> = Vec::new();
        let mut static_fields: Vec<(Rc<str>, Option<HirExpr>)> = Vec::new();
        let mut ctor_member: Option<(&[Param], &Stmt)> = None;
        let mut methods_ast: Vec<(Rc<str>, &[Param], &Stmt, &[Decorator], &Modifiers)> = Vec::new();
        let mut static_methods_ast: Vec<(Rc<str>, &[Param], &Stmt, &[Decorator], &Modifiers)> =
            Vec::new();
        let mut getters_ast: Vec<(Rc<str>, &Stmt, bool)> = Vec::new();
        let mut setters_ast: Vec<(Rc<str>, &Param, &Stmt, bool)> = Vec::new();
        let mut static_blocks_ast: Vec<&Stmt> = Vec::new();
        let mut destructor_ast: Option<&Stmt> = None;

        if let Some(primary_params) = &decl.primary_params {
            for p in primary_params {
                if let varn_core::ast::Pattern::Identifier { name, .. } = &p.pattern {
                    if !fields.contains(name) {
                        fields.push(name.clone());
                    }
                }
            }
        }

        for member in &decl.body {
            match member {
                ClassMember::Property {
                    key,
                    init,
                    modifiers,
                    ..
                } => {
                    if modifiers.is_static {
                        let val = match init {
                            Some(e) => Some(self.lower_expr(e, scope)?),
                            None => None,
                        };
                        static_fields.push((key.clone(), val));
                    } else {
                        if !fields.contains(key) {
                            fields.push(key.clone());
                        }
                        if let Some(e) = init {
                            field_inits.push((key.clone(), e));
                        }
                    }
                }
                ClassMember::Constructor { params, body, .. } => {
                    for p in params {
                        if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                            if let varn_core::ast::Pattern::Identifier { name, .. } = &p.pattern {
                                if !fields.contains(name) {
                                    fields.push(name.clone());
                                }
                            }
                        }
                    }
                    ctor_member = Some((params, body));
                }
                ClassMember::Method {
                    key,
                    params,
                    body: Some(body),
                    modifiers,
                    decorators,
                    ..
                } => {
                    if modifiers.is_static {
                        static_methods_ast.push((key.clone(), params, body, decorators, modifiers));
                    } else {
                        methods_ast.push((key.clone(), params, body, decorators, modifiers));
                    }
                }

                ClassMember::Method { body: None, .. }
                | ClassMember::Getter { body: None, .. }
                | ClassMember::Setter { body: None, .. } => {}
                ClassMember::Getter {
                    key,
                    body: Some(body),
                    modifiers,
                    ..
                } => getters_ast.push((key.clone(), body, modifiers.is_static)),
                ClassMember::Setter {
                    key,
                    param,
                    body: Some(body),
                    modifiers,
                    ..
                } => setters_ast.push((key.clone(), param, body, modifiers.is_static)),
                ClassMember::StaticBlock { body, .. } => static_blocks_ast.push(body),
                ClassMember::Destructor { body, .. } => destructor_ast = Some(body),
            }
        }

        let (ctor_func, ctor_ups) = match ctor_member {
            Some((params, body)) => self.lower_function_like(
                Rc::from("constructor"),
                params,
                false,
                false,
                false,
                true,
                BodyRef::Block(body),
                &field_inits,
                scope,
            )?,
            None => {
                let primary = decl.primary_params.as_deref().unwrap_or(&[]);
                self.lower_function_like(
                    Rc::from("constructor"),
                    primary,
                    false,
                    false,
                    false,
                    true,
                    BodyRef::Empty,
                    &field_inits,
                    scope,
                )?
            }
        };

        let mut methods = Vec::new();
        for (key, params, body, decorators_ast, modifiers) in methods_ast {
            let (func, upvalues) = self.lower_method_fn(
                key.clone(),
                params,
                body,
                true,
                modifiers.is_async,
                modifiers.is_generator,
                scope,
            )?;
            let mut decorators = Vec::new();
            for dec in decorators_ast {
                decorators.push(self.lower_expr(&dec.expression, scope)?);
            }
            let is_private = matches!(modifiers.visibility, Some(Visibility::Private));
            methods.push(HirMethod {
                key,
                func,
                upvalues,
                decorators,
                is_private,
            });
        }

        if let Some(body) = destructor_ast {
            let (func, upvalues) =
                self.lower_method_fn(Rc::from("dispose"), &[], body, true, false, false, scope)?;
            methods.push(HirMethod {
                key: Rc::from("dispose"),
                func,
                upvalues,
                decorators: Vec::new(),
                is_private: false,
            });
        }

        let mut static_methods = Vec::new();
        for (key, params, body, decorators_ast, modifiers) in static_methods_ast {
            let (func, upvalues) = self.lower_method_fn(
                key.clone(),
                params,
                body,
                false,
                modifiers.is_async,
                modifiers.is_generator,
                scope,
            )?;
            let mut decorators = Vec::new();
            for dec in decorators_ast {
                decorators.push(self.lower_expr(&dec.expression, scope)?);
            }
            let is_private = matches!(modifiers.visibility, Some(Visibility::Private));
            static_methods.push(HirMethod {
                key,
                func,
                upvalues,
                decorators,
                is_private,
            });
        }

        let mut getters = Vec::new();
        for (key, body, is_static) in getters_ast {
            let (func, upvalues) =
                self.lower_method_fn(key.clone(), &[], body, !is_static, false, false, scope)?;
            getters.push(HirAccessor {
                key,
                func,
                upvalues,
                is_static,
            });
        }
        let mut setters = Vec::new();
        for (key, param, body, is_static) in setters_ast {
            let (func, upvalues) = self.lower_method_fn(
                key.clone(),
                std::slice::from_ref(param),
                body,
                !is_static,
                false,
                false,
                scope,
            )?;
            setters.push(HirAccessor {
                key,
                func,
                upvalues,
                is_static,
            });
        }

        let mut static_blocks = Vec::new();
        for body in static_blocks_ast {
            let (func, upvalues) = self.lower_method_fn(
                Rc::from("<static_block>"),
                &[],
                body,
                false,
                false,
                false,
                scope,
            )?;
            static_blocks.push(HirMethod {
                key: Rc::from("<static_block>"),
                func,
                upvalues,
                decorators: Vec::new(),
                is_private: false,
            });
        }

        Ok(HirClass {
            name,
            super_class,
            fields,
            static_fields,
            ctor: HirMethod {
                key: Rc::from("constructor"),
                func: ctor_func,
                upvalues: ctor_ups,
                decorators: Vec::new(),
                is_private: false,
            },
            methods,
            static_methods,
            getters,
            setters,
            static_blocks,
            decorators: class_decorators,
        })
    }

    pub(in crate::hir::lower) fn lower_method_fn(
        &mut self,
        key: Rc<str>,
        params: &[Param],
        body: &Stmt,
        has_this: bool,
        is_async: bool,
        is_generator: bool,
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        self.lower_function_like(
            key,
            params,
            is_async,
            is_generator,
            false,
            has_this,
            BodyRef::Block(body),
            &[],
            scope,
        )
    }

    pub(in crate::hir::lower) fn lower_decl_inline(
        &mut self,
        decl: &Decl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<Vec<Rc<str>>> {
        let is_global = scope.is_global();
        match decl {
            Decl::Function(f) => {
                let mut fscope = Scope::new();
                let (func, _ups) = self.lower_function(f, &mut fscope)?;
                let value = HirExpr::Closure {
                    func: Box::new(func),
                    upvalues: Vec::new(),
                };
                if is_global {
                    let target = self.global_binding(f.id.clone());
                    out.push(HirStmt::Assign { target, value });
                } else {
                    let local = scope.alloc_local(f.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value,
                        ty: HirType::Dynamic,
                    });
                }
                Ok(vec![f.id.clone()])
            }
            Decl::Class(cl) => {
                let name = cl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
                let hir_class = self.lower_class(cl, scope)?;
                let value = HirExpr::Class(Box::new(hir_class));
                if is_global {
                    let target = self.global_binding(name.clone());
                    out.push(HirStmt::Assign { target, value });
                } else {
                    let local = scope.alloc_local(name.clone());
                    out.push(HirStmt::Let {
                        local,
                        value,
                        ty: HirType::Dynamic,
                    });
                }
                Ok(vec![name])
            }
            Decl::Enum(en) => {
                let hir_enum = self.lower_enum(en, scope)?;
                let value = HirExpr::Enum(Box::new(hir_enum));
                if is_global {
                    let target = self.global_binding(en.id.clone());
                    out.push(HirStmt::Assign { target, value });
                } else {
                    let local = scope.alloc_local(en.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value,
                        ty: HirType::Dynamic,
                    });
                }
                Ok(vec![en.id.clone()])
            }
            Decl::SumType(st) => {
                let hir_enum = self.lower_sum_type(st, scope)?;
                let value = HirExpr::Enum(Box::new(hir_enum));
                let mut bound = vec![st.id.clone()];
                if is_global {
                    let target = self.global_binding(st.id.clone());
                    out.push(HirStmt::Assign { target, value });
                    for v in &st.variants {
                        let target = self.global_binding(v.name.clone());
                        let st_global = self.global_binding(st.id.clone());
                        out.push(HirStmt::Assign {
                            target,
                            value: HirExpr::Member {
                                object: Box::new(HirExpr::Var(st_global)),
                                name: v.name.clone(),
                                ty: HirType::Dynamic,
                            },
                        });
                        bound.push(v.name.clone());
                    }
                } else {
                    let local = scope.alloc_local(st.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value,
                        ty: HirType::Dynamic,
                    });
                    for v in &st.variants {
                        let vlocal = scope.alloc_local(v.name.clone());
                        out.push(HirStmt::Let {
                            local: vlocal,
                            value: HirExpr::Member {
                                object: Box::new(HirExpr::Var(HirBinding::Local(local))),
                                name: v.name.clone(),
                                ty: HirType::Dynamic,
                            },
                            ty: HirType::Dynamic,
                        });
                        bound.push(v.name.clone());
                    }
                }
                Ok(bound)
            }
            Decl::Variable(v) => {
                let mut names = Vec::new();
                for d in &v.declarators {
                    let value = match &d.init {
                        Some(e) => self.lower_expr(e, scope)?,
                        None => HirExpr::Null,
                    };
                    if is_global {
                        self.desugar_pattern_global(&d.id, value, scope, out)?;
                    } else {
                        self.desugar_pattern_local(&d.id, value, scope, out)?;
                    }
                    let before = names.len();
                    collect_pattern_identifiers(&d.id, &mut names);
                    // Candidate for `<module>` register promotion. Exported
                    // names are read by other modules through their slot, and
                    // namespace members are read back to build the namespace
                    // object, so both stay globals.
                    if is_global && !self.in_namespace {
                        for n in &names[before..] {
                            if !self.export_names.contains(n) {
                                if let HirBinding::Global(q) = self.global_binding(n.clone()) {
                                    self.top_level_lets.push(q);
                                }
                            }
                        }
                    }
                }
                Ok(names)
            }
            Decl::Namespace(ns) => {
                self.lower_namespace(ns, scope, out)?;
                Ok(vec![ns.id.clone()])
            }
            Decl::Extension(ext) => {
                self.lower_extension(ext, scope, out)?;
                Ok(Vec::new())
            }
            Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => Ok(Vec::new()),
            _ => return Err(OptError::Unsupported("hir: inline decl kind")),
        }
    }
}
