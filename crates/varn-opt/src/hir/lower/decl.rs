//! Declaration-level AST→HIR lowering: functions, classes, enums, imports,
//! exports, and the shared function-like body builder.

use std::rc::Rc;

use varn_core::ast::decl::{ClassDecl, ClassMember, EnumDecl, ExportDecl, ImportDecl, ImportSpecifier};
use varn_core::ast::{Decl, Expr, ExprKind, FunctionDecl, Param, Pattern, Stmt, StmtKind};

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

    /// Lower a class declaration to a `HirClass` (core subset: no inheritance,
    /// static members, accessors, or decorators — those fall back to legacy).
    pub(super) fn lower_class(&mut self, decl: &ClassDecl, scope: &mut Scope) -> R<HirClass> {
        let name = decl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
        if decl.super_class.is_some() {
            return unsupported("class inheritance");
        }
        if !decl.decorators.is_empty() {
            return unsupported("class decorators");
        }
        // Type parameters are erased at codegen — they don't block lowering.
        if decl.modifiers.is_abstract {
            return unsupported("abstract class");
        }

        let mut fields: Vec<Rc<str>> = Vec::new();
        let mut field_inits: Vec<(Rc<str>, &Expr)> = Vec::new();
        let mut ctor_member: Option<(&[Param], &Stmt)> = None;
        let mut methods_ast: Vec<(Rc<str>, &[Param], &Stmt)> = Vec::new();

        for member in &decl.body {
            match member {
                ClassMember::Property {
                    key,
                    init,
                    modifiers,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static field");
                    }
                    fields.push(key.clone());
                    if let Some(e) = init {
                        field_inits.push((key.clone(), e));
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
                        return unsupported("static method");
                    }
                    if modifiers.is_async || modifiers.is_generator {
                        return unsupported("async/generator method");
                    }
                    if !decorators.is_empty() {
                        return unsupported("method decorators");
                    }
                    methods_ast.push((key.clone(), params, body));
                }
                ClassMember::Method { body: None, .. } => return unsupported("abstract method"),
                ClassMember::Getter { .. } | ClassMember::Setter { .. } => {
                    return unsupported("class accessor")
                }
                ClassMember::StaticBlock { .. } => return unsupported("static block"),
                ClassMember::Destructor { .. } => return unsupported("class destructor"),
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
        for (key, params, body) in methods_ast {
            let (func, upvalues) = self.lower_function_like(
                key.clone(),
                params,
                false,
                false,
                false,
                true,
                BodyRef::Block(body),
                &[],
                scope,
            )?;
            methods.push(HirMethod {
                key,
                func,
                upvalues,
            });
        }

        Ok(HirClass {
            name,
            fields,
            ctor: HirMethod {
                key: Rc::from("constructor"),
                func: ctor_func,
                upvalues: ctor_ups,
            },
            methods,
        })
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
        match decl {
            Decl::Function(f) => {
                let mut fscope = Scope::new();
                let (func, _ups) = self.lower_function(f, &mut fscope)?;
                out.push(HirStmt::Assign {
                    target: HirBinding::Global(f.id.clone()),
                    value: HirExpr::Closure {
                        func: Box::new(func),
                        upvalues: Vec::new(),
                    },
                });
                Ok(vec![f.id.clone()])
            }
            Decl::Class(cl) => {
                let name = match &cl.id {
                    Some(id) => id.clone(),
                    None => return unsupported("anonymous exported class"),
                };
                let hir_class = self.lower_class(cl, scope)?;
                out.push(HirStmt::Assign {
                    target: HirBinding::Global(name.clone()),
                    value: HirExpr::Class(Box::new(hir_class)),
                });
                Ok(vec![name])
            }
            Decl::Enum(en) => {
                let hir_enum = self.lower_enum(en, scope)?;
                out.push(HirStmt::Assign {
                    target: HirBinding::Global(en.id.clone()),
                    value: HirExpr::Enum(Box::new(hir_enum)),
                });
                Ok(vec![en.id.clone()])
            }
            Decl::Variable(v) => {
                let mut names = Vec::new();
                for d in &v.declarators {
                    let name = match &d.id {
                        Pattern::Identifier { name, .. } => name.clone(),
                        _ => return unsupported("destructuring export/member"),
                    };
                    let value = match &d.init {
                        Some(e) => self.lower_expr(e, scope)?,
                        None => HirExpr::Null,
                    };
                    out.push(HirStmt::Assign {
                        target: HirBinding::Global(name.clone()),
                        value,
                    });
                    names.push(name);
                }
                Ok(names)
            }
            Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => Ok(Vec::new()),
            _ => unsupported("inline decl kind"),
        }
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
            _ => unsupported("export default/named/all"),
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
    /// a class; instance fields/methods mirror the class core. (Core subset: no
    /// static members, field initializers, or accessors.)
    pub(super) fn lower_enum(&mut self, decl: &EnumDecl, scope: &mut Scope) -> R<HirEnum> {
        // Type parameters are erased at codegen — they don't block lowering.
        let name = decl.id.clone();
        let mut variants = Vec::new();
        let mut tag = 0i64;
        for member in &decl.members {
            if let Some(init) = &member.init {
                match &init.kind {
                    ExprKind::IntLiteral { value, .. } => tag = *value,
                    _ => return unsupported("non-integer enum discriminant"),
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
            variants.push(HirEnumVariant {
                name: member.id.clone(),
                tag,
                meta: Rc::from(meta.as_str()),
            });
            tag += 1;
        }

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        for member in &decl.body {
            match member {
                ClassMember::Property {
                    key,
                    init,
                    modifiers,
                    ..
                } => {
                    if modifiers.is_static {
                        return unsupported("static enum field");
                    }
                    if init.is_some() {
                        return unsupported("enum field initializer");
                    }
                    fields.push(key.clone());
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
                        return unsupported("static enum method");
                    }
                    if modifiers.is_async || modifiers.is_generator {
                        return unsupported("async/generator enum method");
                    }
                    if !decorators.is_empty() {
                        return unsupported("enum method decorators");
                    }
                    let (func, upvalues) = self.lower_function_like(
                        key.clone(),
                        params,
                        false,
                        false,
                        false,
                        true,
                        BodyRef::Block(body),
                        &[],
                        scope,
                    )?;
                    methods.push(HirMethod {
                        key: key.clone(),
                        func,
                        upvalues,
                    });
                }
                _ => return unsupported("enum member kind"),
            }
        }

        Ok(HirEnum {
            name,
            variants,
            fields,
            methods,
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
        generic: bool,
        has_this: bool,
        body: BodyRef<'_>,
        field_inits: &[(Rc<str>, &Expr)],
        scope: &mut Scope,
    ) -> R<(HirFunction, Vec<HirUpvalueSrc>)> {
        if is_async || is_generator {
            return unsupported("async/generator function");
        }
        if generic {
            return unsupported("generic function");
        }
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
        let mut params = Vec::new();
        for (i, p) in params_ast.iter().enumerate() {
            if p.is_rest || p.is_optional || p.default.is_some() {
                return unsupported("rest/optional/default param");
            }
            let pname = match &p.pattern {
                Pattern::Identifier { name, .. } => name.clone(),
                _ => return unsupported("destructuring param"),
            };
            scope.define(pname.clone(), HirBinding::Param(i as u32));
            params.push(HirParam {
                name: pname,
                ty: param_ty(p),
            });
        }
        let mut out = Vec::new();
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
