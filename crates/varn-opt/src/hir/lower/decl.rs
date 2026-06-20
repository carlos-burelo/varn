//! Declaration-level AST→HIR lowering: functions, classes, enums, imports,
//! exports, and the shared function-like body builder.

use std::rc::Rc;

use varn_core::ast::decl::{
    ClassDecl, ClassMember, EnumDecl, ExportDecl, ExportDefaultDecl, ExtensionDecl, ExtensionMember, ImportDecl,
    ImportSpecifier, NamespaceDecl, SumTypeDecl,
};
use varn_core::ast::{
    Decl, Decorator, Expr, ExprKind, FunctionDecl, Modifiers, Param, Pattern, Stmt, StmtKind, TypeNode,
    Visibility,
};

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a function declaration into its `HirFunction` plus the upvalues it
    /// captures from enclosing frames. `scope` carries the enclosing frame
    /// stack so nested functions can capture; top-level callers pass a fresh
    /// `Scope` (its empty frame 0 yields no captures → globals).
    pub(super) fn lower_function(
        &mut self,
        f: &FunctionDecl,
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        self.lower_function_like(
            f.id.clone(),
            &f.params,
            f.modifiers.is_async,
            f.modifiers.is_generator,
            // Type parameters are erased at codegen — they don't block lowering.
            false,
            false,
            BodyRef::Block(&f.body),
            &[],
            scope,
        )
    }

    /// Lower a class declaration to a `HirClass`. Covers fields, constructor,
    /// instance/static methods, getters/setters, static blocks, and destructor.
    /// `abstract` and type parameters are erased; inheritance and decorators
    /// still fall back to legacy.
    pub(super) fn lower_class(&mut self, decl: &ClassDecl, scope: &mut Scope) -> R<HirClass> {
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
        let mut static_methods_ast: Vec<(Rc<str>, &[Param], &Stmt, &[Decorator], &Modifiers)> = Vec::new();
        let mut getters_ast: Vec<(Rc<str>, &Stmt, bool)> = Vec::new();
        let mut setters_ast: Vec<(Rc<str>, &Param, &Stmt, bool)> = Vec::new();
        let mut static_blocks_ast: Vec<&Stmt> = Vec::new();
        let mut destructor_ast: Option<&Stmt> = None;

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
                // Abstract method/accessor (no body): declares nothing at runtime.
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

        // Constructor (synthesised when absent); field inits run after its body.
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
            let is_private = matches!(modifiers.visibility, Some(Visibility::Private));
            methods.push(HirMethod {
                key,
                func,
                upvalues,
                decorators,
                is_private,
            });
        }
        // Destructor → an instance method bound as `dispose`.
        if let Some(body) = destructor_ast {
            let (func, upvalues) = self.lower_method_fn(
                Rc::from("dispose"),
                &[],
                body,
                true,
                false,
                false,
                scope,
            )?;
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
            let (func, upvalues) = self.lower_method_fn(
                key.clone(),
                &[],
                body,
                !is_static,
                false,
                false,
                scope,
            )?;
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

    /// Lower a class method/accessor/static-block body to a function + upvalues.
    /// `has_this` is false for statics and static blocks.
    fn lower_method_fn(
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

    /// Lower a declaration that defines module global(s) inline, emitting the
    /// defining statements into `out` and returning the names it bound. Used for
    /// `export <decl>` and namespace members.
    fn lower_decl_inline(
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
                    out.push(HirStmt::Assign {
                        target: HirBinding::Global(f.id.clone()),
                        value,
                    });
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
                let name = match &cl.id {
                    Some(id) => id.clone(),
                    None => panic!("anonymous class declaration has no name"),
                };
                let hir_class = self.lower_class(cl, scope)?;
                let value = HirExpr::Class(Box::new(hir_class));
                if is_global {
                    out.push(HirStmt::Assign {
                        target: HirBinding::Global(name.clone()),
                        value,
                    });
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
                    out.push(HirStmt::Assign {
                        target: HirBinding::Global(en.id.clone()),
                        value,
                    });
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
                    out.push(HirStmt::Assign {
                        target: HirBinding::Global(st.id.clone()),
                        value,
                    });
                    for v in &st.variants {
                        out.push(HirStmt::Assign {
                            target: HirBinding::Global(v.name.clone()),
                            value: HirExpr::Member {
                                object: Box::new(HirExpr::Var(HirBinding::Global(st.id.clone()))),
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
                    collect_pattern_identifiers(&d.id, &mut names);
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
            _ => panic!("unsupported inline decl kind: {:?}", decl),
        }
    }

    /// Lower `namespace N { members }`: each member is defined as a module
    /// global, then an object `{ name: <global>, … }` is bound to global `N`.
    /// Mirrors legacy `compile_namespace_decl` — members are globals (last-write
    /// wins on name clashes), and only `function`/`class`/`var` members become
    /// object properties (enum/nested-namespace members are defined but not
    /// captured, matching legacy).
    pub(super) fn lower_namespace(
        &mut self,
        ns: &NamespaceDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        let is_global = scope.is_global();
        scope.push_block();
        let mut names = Vec::new();
        for member in &ns.body {
            let inner = match member {
                Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
                other => other,
            };
            let bound = self.lower_decl_inline(inner, scope, out)?;
            if matches!(
                inner,
                Decl::Function(_) | Decl::Class(_) | Decl::Variable(_) | Decl::Namespace(_) | Decl::Enum(_) | Decl::SumType(_)
            ) {
                names.extend(bound);
            }
        }
        let properties = names
            .into_iter()
            .map(|name| {
                let binding = scope.resolve_in_current_frame(&name)
                    .unwrap_or(HirBinding::Global(name.clone()));
                let value = HirExpr::Var(binding);
                HirObjectProp::Property {
                    key: HirPropKey::Static(name),
                    value,
                }
            })
            .collect();
        scope.pop_block();
        let value = HirExpr::Object { properties };
        if is_global {
            out.push(HirStmt::Assign {
                target: HirBinding::Global(ns.id.clone()),
                value,
            });
        } else {
            let local = scope.alloc_local(ns.id.clone());
            out.push(HirStmt::Let {
                local,
                value,
                ty: HirType::Dynamic,
            });
        }
        Ok(())
    }

    /// Lower an `extension Target { … }` declaration: each member becomes a
    /// mangled global closure (`__ext_T_m` / `__extget_T_k` / `__extset_T_k`,
    /// all `has_this`), exactly as legacy `compile_extension_decl`. Uses
    /// (`x.m()`, `x.k`, `x.k = v`) are lowered to plain calls of these globals
    /// via the checker's `extension_calls`/`_members`/`_set_members` maps.
    pub(super) fn lower_extension(
        &mut self,
        ext: &ExtensionDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        let ty = extension_type_name(&ext.target);
        for member in &ext.members {
            match member {
                ExtensionMember::Method(m) => {
                    if m.modifiers.is_async || m.modifiers.is_generator {
                        panic!("async/generator extension method is unsupported");
                    }
                    let mangled: Rc<str> = Rc::from(format!("__ext_{}_{}", ty, m.id));
                    self.push_global_closure(out, mangled, &m.params, &m.body, scope)?;
                }
                ExtensionMember::Getter { key, body, .. } => {
                    let mangled: Rc<str> = Rc::from(format!("__extget_{ty}_{key}"));
                    self.push_global_closure(out, mangled, &[], body, scope)?;
                }
                ExtensionMember::Setter {
                    key, param, body, ..
                } => {
                    let mangled: Rc<str> = Rc::from(format!("__extset_{ty}_{key}"));
                    self.push_global_closure(
                        out,
                        mangled,
                        std::slice::from_ref(param),
                        body,
                        scope,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Lower a `has_this` function body and bind it as a module global closure.
    fn push_global_closure(
        &mut self,
        out: &mut Vec<HirStmt>,
        name: Rc<str>,
        params: &[Param],
        body: &Stmt,
        scope: &mut Scope,
    ) -> R<()> {
        let (func, upvalues) = self.lower_function_like(
            name.clone(),
            params,
            false,
            false,
            false,
            true,
            BodyRef::Block(body),
            &[],
            scope,
        )?;
        out.push(HirStmt::Assign {
            target: HirBinding::Global(name),
            value: HirExpr::Closure {
                func: Box::new(func),
                upvalues,
            },
        });
        Ok(())
    }

    pub(super) fn lower_import(&self, decl: &ImportDecl) -> R<HirStmt> {
        let mut specs = Vec::new();
        for spec in &decl.specifiers {
            let (local, kind, off) = match spec {
                ImportSpecifier::Default { local, range } => {
                    (local.clone(), HirImportKind::Default, range.start.offset)
                }
                ImportSpecifier::Named {
                    local,
                    imported,
                    range,
                } => (
                    local.clone(),
                    HirImportKind::Named(imported.clone()),
                    range.start.offset,
                ),
                ImportSpecifier::Namespace { local, range } => {
                    (local.clone(), HirImportKind::Namespace, range.start.offset)
                }
            };
            let slot = self.ann.get_slot_idx(off).map(|s| s as u16);
            specs.push(HirImportSpec { local, kind, slot });
        }
        Ok(HirStmt::Import {
            source: decl.source.clone(),
            is_type: decl.is_type,
            specs,
        })
    }

    pub(super) fn lower_export(
        &mut self,
        decl: &ExportDecl,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match decl {
            ExportDecl::Decl { declaration, .. } => {
                let off = declaration.range().start.offset;
                let names = self.lower_decl_inline(declaration, scope, out)?;
                for name in names {
                    if let Some(slot) = self.export_slot(&name, off) {
                        out.push(HirStmt::StoreExport { name, slot });
                    }
                }
                Ok(())
            }
            ExportDecl::Default {
                declaration, range, ..
            } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    if !f.modifiers.is_declare {
                        let local = scope.alloc_local(f.id.clone());
                        let (func, upvalues) = self.lower_function(f, scope)?;
                        out.push(HirStmt::Let {
                            local,
                            value: HirExpr::Closure {
                                func: Box::new(func),
                                upvalues,
                            },
                            ty: HirType::Dynamic,
                        });
                        let slot = self.export_names
                            .iter()
                            .position(|n| &**n == "default")
                            .or_else(|| self.ann.get_slot_idx(range.start.offset))
                            .map(|p| p as u16);
                        if let Some(slot) = slot {
                            out.push(HirStmt::StoreExport { name: f.id.clone(), slot });
                        }
                    }
                    Ok(())
                }
                ExportDefaultDecl::Class(cl) => {
                    if !cl.modifiers.is_declare {
                        let name = cl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
                        let hir_class = self.lower_class(cl, scope)?;
                        let value = HirExpr::Class(Box::new(hir_class));
                        if scope.is_global() {
                            out.push(HirStmt::Assign {
                                target: HirBinding::Global(name.clone()),
                                value,
                            });
                        } else {
                            let local = scope.alloc_local(name.clone());
                            out.push(HirStmt::Let {
                                local,
                                value,
                                ty: HirType::Dynamic,
                            });
                        }
                        let slot = self.export_names
                            .iter()
                            .position(|n| &**n == "default")
                            .or_else(|| self.ann.get_slot_idx(range.start.offset))
                            .map(|p| p as u16);
                        if let Some(slot) = slot {
                            out.push(HirStmt::StoreExport { name, slot });
                        }
                    }
                    Ok(())
                }
                ExportDefaultDecl::Expr(e) => {
                    let value = self.lower_expr(e, scope)?;
                    let slot = self.export_names
                        .iter()
                        .position(|n| &**n == "default")
                        .or_else(|| self.ann.get_slot_idx(range.start.offset))
                        .map(|p| p as u16);
                    out.push(HirStmt::ExportDefaultExpr { value, slot });
                    Ok(())
                }
            },
            ExportDecl::Named {
                specifiers,
                source,
                range: _,
                ..
            } => {
                let mut specs = Vec::new();
                for spec in specifiers {
                    let binding = self.resolve(&spec.local, scope);
                    let local_slot = self.ann.get_slot_idx(spec.range.start.offset).map(|s| s as u16);
                    let exported_slot = self.export_names
                        .iter()
                        .position(|n| &**n == &*spec.exported)
                        .or_else(|| self.ann.get_slot_idx(spec.range.start.offset))
                        .map(|s| s as u16);
                    specs.push(HirExportSpec {
                        binding,
                        local: spec.local.clone(),
                        exported: spec.exported.clone(),
                        local_slot,
                        exported_slot,
                    });
                }
                out.push(HirStmt::ExportNamed {
                    specifiers: specs,
                    source: source.clone(),
                });
                Ok(())
            }
            ExportDecl::All {
                source,
                alias,
                range,
                ..
            } => {
                let slot = alias.as_ref().and_then(|alias_name| {
                    self.export_names
                        .iter()
                        .position(|n| **n == **alias_name)
                        .or_else(|| self.ann.get_slot_idx(range.start.offset))
                        .map(|s| s as u16)
                });
                out.push(HirStmt::ExportAll {
                    source: source.clone(),
                    alias: alias.clone(),
                    slot,
                });
                Ok(())
            }
        }
    }

    fn export_slot(&self, name: &str, offset: u32) -> Option<u16> {
        self.export_names
            .iter()
            .position(|n| **n == *name)
            .map(|p| p as u16)
            .or_else(|| self.ann.get_slot_idx(offset).map(|s| s as u16))
    }

    /// Lower an enum declaration. Variants become `MakeEnumVariant` statics on
    /// a class; instance fields/methods mirror the class core.
    pub(super) fn lower_enum(&mut self, decl: &EnumDecl, scope: &mut Scope) -> R<HirEnum> {
        // Type parameters are erased at codegen — they don't block lowering.
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
                match &f.ty.kind {
                    varn_core::TypeKind::LiteralInt(val) => {
                        const_args.push(HirExpr::Int(*val));
                    }
                    varn_core::TypeKind::LiteralStr(val) => {
                        const_args.push(HirExpr::Str(Rc::from(val.as_str())));
                    }
                    varn_core::TypeKind::LiteralFloat(bits) => {
                        const_args.push(HirExpr::Float(f64::from_bits(*bits)));
                    }
                    varn_core::TypeKind::LiteralBool(val) => {
                        const_args.push(HirExpr::Bool(*val));
                    }
                    _ => {}
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
        let mut static_methods_ast: Vec<(Rc<str>, &[Param], &Stmt, &[Decorator], &Modifiers)> = Vec::new();
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
            let (func, upvalues) = self.lower_method_fn(
                key.clone(),
                &[],
                body,
                !is_static,
                false,
                false,
                scope,
            )?;
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

    pub(super) fn lower_sum_type(&mut self, decl: &SumTypeDecl, scope: &mut Scope) -> R<HirEnum> {
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

    /// Shared lowering for declarations, function expressions, arrows, and class
    /// methods/constructors. `has_this` marks register 0 as the receiver;
    /// `field_inits` are `this.name = expr` assignments appended after the body
    /// (constructor field initializers, lowered in the constructor's frame —
    /// matching legacy ordering).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_function_like(
        &mut self,
        name: Rc<str>,
        params_ast: &[Param],
        is_async: bool,
        is_generator: bool,
        _generic: bool,
        has_this: bool,
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {

        scope.push_frame();
        let prev_fn = self.current_fn.take();
        self.current_fn = Some(name.clone());

        let built = self.lower_function_body(params_ast, body, field_inits, scope);

        self.current_fn = prev_fn;
        let (params, mut body) = match built {
            Ok(v) => v,
            Err(e) => {
                scope.pop_frame(); // keep the frame stack balanced on error
                return Err(e);
            }
        };
        // Function-level captures (params + top-level locals) are closed by the
        // VM's `Return` (close_upvalues_above base); only inner blocks need an
        // explicit `CloseUpvalue`, emitted in `lower_block`/`Block`/`For`.
        // Function-level `using` resources are disposed at the body's end
        // (reverse declaration order), matching legacy `pop_scope`.
        let (locals, upvalues, _captured0, disposables0) = scope.pop_frame();
        for (target, is_await) in disposables0.into_iter().rev() {
            body.push(HirStmt::Dispose { target, is_await });
        }
        let func = HirFunction {
            name,
            params,
            locals,
            body,
            return_ty: HirType::Dynamic,
            upvalue_count: upvalues.len() as u32,
            has_this,
            has_rest: params_ast.iter().any(|p| p.is_rest),
            is_async,
            is_generator,
        };
        Ok((func, upvalues))
    }

    fn lower_function_body(
        &mut self,
        params_ast: &[Param],
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(Vec<HirParam>, Vec<HirStmt>)> {
        // Pass 1: bind every param name so defaults (lowered below) can refer to
        // earlier params, exactly like the legacy child compiler which collects
        // `param_regs` before running the default prologue.
        let mut params = Vec::new();
        let mut destructuring_params = Vec::new();
        for (i, p) in params_ast.iter().enumerate() {
            let pname = match &p.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => {
                    let name = Rc::from(format!("__p{}", i + 1));
                    destructuring_params.push((i, &p.pattern));
                    name
                }
            };
            scope.define(pname.clone(), HirBinding::Param(i as u32));
            params.push(HirParam {
                name: pname,
                ty: param_ty(p),
                default: None,
            });
        }
        // Pass 2: lower default expressions (`x = expr`). `is_optional` without a
        // default needs nothing — the VM passes null when the arg is absent; rest
        // params are handled via `HirFunction.has_rest`.
        for (i, p) in params_ast.iter().enumerate() {
            if let Some(default) = &p.default {
                params[i].default = Some(self.lower_expr(default, scope)?);
            }
        }
        let mut out = Vec::new();
        for (i, pat) in destructuring_params {
            self.desugar_pattern_local(pat, HirExpr::Var(HirBinding::Param(i as u32)), scope, &mut out)?;
        }
        match body {
            BodyRef::Block(stmt) => match &stmt.kind {
                StmtKind::Block { stmts } => {
                    for s in stmts {
                        self.lower_stmt(s, scope, &mut out)?;
                    }
                }
                _ => self.lower_stmt(stmt, scope, &mut out)?,
            },
            BodyRef::ExprBody(e) => {
                let v = self.lower_expr(e, scope)?;
                out.push(HirStmt::Return(Some(v)));
            }
            BodyRef::Empty => {}
        }
        // Constructor field initializers run after the body (legacy order).
        for (fname, fexpr) in field_inits {
            let value = self.lower_expr(fexpr, scope)?;
            out.push(HirStmt::SetMember {
                object: HirExpr::This,
                name: fname.clone(),
                value,
            });
        }
        Ok((params, out))
    }
}

/// The mangled type prefix for an extension target (`int`, `str`, a class name,
/// …). Mirrors legacy `compile_extension_decl` so the generated global names
/// match the checker's `extension_*` maps used at the call/member sites.
fn extension_type_name(target: &TypeNode) -> String {
    use varn_core::{IntrinsicType, TypeKind, TypeTag};
    match &target.kind {
        TypeKind::Intrinsic(TypeTag::Int) => IntrinsicType::Int.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Float) => IntrinsicType::Float.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Str) => IntrinsicType::Str.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Bool) => IntrinsicType::Bool.as_str().to_owned(),
        TypeKind::Intrinsic(TypeTag::Char) => IntrinsicType::Char.as_str().to_owned(),
        TypeKind::Named(n, _) => n.clone(),
        TypeKind::Generic(n, _, _) => n.clone(),
        TypeKind::Intrinsic(TypeTag::Array) => IntrinsicType::Array.as_str().to_owned(),
        _ => IntrinsicType::Dynamic.as_str().to_owned(),
    }
}
