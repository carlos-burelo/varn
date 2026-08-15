//! `enum` and sum-type declarations.

use std::rc::Rc;

use varn_core::ast::decl::{ClassMember, EnumDecl, SumTypeDecl};
use varn_core::ast::{Decorator, Expr, ExprKind, Modifiers, Param, Stmt, Visibility};

use super::super::*;

impl<'a> Lowerer<'a> {
    pub(in crate::hir::lower) fn lower_enum(
        &mut self,
        decl: &EnumDecl,
        scope: &mut Scope,
    ) -> R<HirEnum> {
        let name = decl.id.clone();
        let mut variants = Vec::new();
        let mut tag = 0i64;
        for member in &decl.members {
            if let Some(init) = &member.init {
                if let ExprKind::IntLiteral { value, .. } = &init.kind {
                    tag = *value;
                }
            }
            let fields_str = member
                .payload_fields
                .iter()
                .map(|f| f.name.as_ref())
                .collect::<Vec<&str>>()
                .join(",");
            let meta = if fields_str.is_empty() {
                format!("{}.{}", name, member.id)
            } else {
                format!("{}.{}:{}", name, member.id, fields_str)
            };
            let mut const_args = Vec::new();
            for f in &member.payload_fields {
                if let Some(init) = &f.init {
                    let hir_expr = self.lower_expr(init, scope)?;
                    const_args.push(hir_expr);
                }
            }
            variants.push(HirEnumVariant {
                name: member.id.clone(),
                tag,
                meta: Rc::from(meta.as_str()),
                const_args,
            });
            tag += 1;
        }

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
                        fields.push(key.clone());
                        if let Some(e) = init {
                            field_inits.push((key.clone(), e));
                        }
                    }
                }
                ClassMember::Constructor { params, body, .. } => {
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
                ClassMember::Destructor { .. } => {}
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
            None => self.lower_function_like(
                Rc::from("constructor"),
                &[],
                false,
                false,
                false,
                true,
                BodyRef::Empty,
                &field_inits,
                scope,
            )?,
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
            methods.push(HirMethod {
                key,
                func,
                upvalues,
                decorators,
                is_private: matches!(modifiers.visibility, Some(Visibility::Private)),
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
            static_methods.push(HirMethod {
                key,
                func,
                upvalues,
                decorators,
                is_private: matches!(modifiers.visibility, Some(Visibility::Private)),
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

        Ok(HirEnum {
            name,
            variants,
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
        })
    }

    pub(in crate::hir::lower) fn lower_sum_type(
        &mut self,
        decl: &SumTypeDecl,
        scope: &mut Scope,
    ) -> R<HirEnum> {
        let name = decl.id.clone();
        let mut variants = Vec::new();
        for (tag, variant) in decl.variants.iter().enumerate() {
            let fields_str = variant
                .fields
                .iter()
                .map(|f| f.name.as_ref())
                .collect::<Vec<&str>>()
                .join(",");
            let meta = if fields_str.is_empty() {
                format!("{}.{}", name, variant.name)
            } else {
                format!("{}.{}:{}", name, variant.name, fields_str)
            };
            variants.push(HirEnumVariant {
                name: variant.name.clone(),
                tag: tag as i64,
                meta: Rc::from(meta.as_str()),
                const_args: Vec::new(),
            });
        }
        let (ctor_func, ctor_ups) = self.lower_function_like(
            Rc::from("constructor"),
            &[],
            false,
            false,
            false,
            true,
            BodyRef::Empty,
            &[],
            scope,
        )?;
        Ok(HirEnum {
            name,
            variants,
            fields: Vec::new(),
            static_fields: Vec::new(),
            ctor: HirMethod {
                key: Rc::from("constructor"),
                func: ctor_func,
                upvalues: ctor_ups,
                decorators: Vec::new(),
                is_private: false,
            },
            methods: Vec::new(),
            static_methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_blocks: Vec::new(),
        })
    }
}
