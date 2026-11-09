use std::sync::Arc;
use varn_core::ast::{ExtensionDecl, ExtensionMember};
use varn_sem::scope::ScopeKind;
use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::Type;

impl<'r> super::super::Binder<'r> {
    fn define_outside(
        &mut self,
        scope: varn_sem::scope::ScopeId,
        name: varn_core::Atom,
        sym: Symbol,
    ) -> varn_sem::symbol::SymbolId {
        let inner = self.current;
        self.current = scope;
        let id = self.define(name, sym);
        self.current = inner;
        id
    }

    pub(crate) fn bind_extension(&mut self, e: &ExtensionDecl) {
        let outer = self.current;
        self.current = self.scopes.child(ScopeKind::Function, outer);
        self.bind_type_params(&e.type_params, e.range.start.line);
        self.bind_extension_in_scope(e, outer);
        self.current = outer;
    }

    fn bind_extension_in_scope(&mut self, e: &ExtensionDecl, outer: varn_sem::scope::ScopeId) {
        let ext_params: Vec<Arc<str>> = e
            .type_params
            .iter()
            .map(|t| Arc::from(self.interner.resolve(t.name)))
            .collect();
        let receiver_ty = self.resolve_type(&e.target);
        let Some(type_name) = receiver_ty.extension_key(&self.ty_table, &|atom| {
            self.interner
                .try_resolve(atom)
                .or_else(|| self.ty_table.name(atom))
                .map(Arc::from)
        }) else {
            self.emit(
                varn_core::Diagnostic::error(
                    varn_core::ErrorCode::InvalidTypeOperator,
                    "an extension can only target a named type, a primitive, an array, a tuple or a record",
                )
                .with_range(e.target.range),
            );
            return;
        };
        let type_name = type_name.to_string();

        if let Some(id) = e.id {
            let line = e.range.start.line;
            let ext_type = receiver_ty;
            let mut sym = Symbol::new(SymbolKind::Extension, id, line).with_type(ext_type);
            sym.col = e.range.start.column;
            sym.offset = e.range.start.offset;
            self.define_outside(outer, id, sym);
        }

        for member in &e.members {
            match member {
                ExtensionMember::Method(method) => {
                    let mangled = varn_core::extension_names::ExtensionKind::Method
                        .mangled(&type_name, self.interner.resolve(method.id));
                    let mut param_types: Vec<varn_sem::types::FunctionParam> =
                        vec![varn_sem::types::FunctionParam {
                            name: Some(Arc::from(varn_core::TokenKind::This.as_str())),
                            ty: receiver_ty.0,
                            optional: false,
                            is_rest: false,
                        }];
                    for p in &method.params {
                        let ty = self.param_type(p, crate::ParamSite::Declared);
                        param_types.push(varn_sem::types::FunctionParam {
                            name: Some(Arc::from(
                                varn_sem::infer::pattern_to_string(
                                    &p.pattern,
                                    Some(&self.interner),
                                )
                                .as_str(),
                            )),
                            ty: ty.0,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        });
                    }
                    let declared_ret = method
                        .return_type
                        .as_ref()
                        .map(|rt| self.resolve_type(rt))
                        .unwrap_or(Type::Void);
                    let ret_ty = varn_sem::types::async_fn_return(
                        declared_ret,
                        method.modifiers.is_async,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let fn_type = Type::fn_(
                        varn_sem::types::FunctionType {
                            params: param_types,
                            return_type: ret_ty.0,
                            is_arrow: false,
                            type_params: ext_params
                                .iter()
                                .cloned()
                                .chain(
                                    method
                                        .type_params
                                        .iter()
                                        .map(|t| Arc::from(self.interner.resolve(t.name))),
                                )
                                .collect(),
                            type_param_defaults: Vec::new(),
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let line = method.range.start.line;
                    let mangled_atom = self.intern_local(&mangled);
                    let mut sym =
                        Symbol::new(SymbolKind::Function, mangled_atom, line).with_type(fn_type);
                    sym.col = method.range.start.column;
                    sym.offset = method.range.start.offset;
                    sym.has_explicit_type = method.return_type.is_some();
                    sym.is_async = method.modifiers.is_async;
                    sym.is_generator = method.modifiers.is_generator;
                    self.define_outside(outer, mangled_atom, sym);
                    if method.modifiers.is_declare {
                        self.extensions
                            .forwarded
                            .insert(Arc::from(mangled.as_str()));
                    }
                    self.extensions
                        .methods
                        .entry(Arc::from(type_name.as_str()))
                        .or_default()
                        .insert(
                            Arc::from(self.interner.resolve(method.id)),
                            Arc::from(mangled.as_str()),
                        );
                    self.bind_extension_function_scope(
                        line,
                        receiver_ty,
                        &method.params,
                        method.body,
                    );
                }
                ExtensionMember::Getter {
                    key,
                    return_type,
                    body,
                    range,
                    ..
                } => {
                    let key_str = self.interner.resolve(*key).to_string();
                    let mangled = varn_core::extension_names::ExtensionKind::Getter
                        .mangled(&type_name, &key_str);
                    let ret_ty = match return_type {
                        Some(rt) => self.resolve_type(rt),
                        None => {
                            self.emit(
                                varn_core::Diagnostic::error(
                                    varn_core::ErrorCode::TypeAnnotationRequired,
                                    format!("extension getter '{key_str}' needs a return type annotation"),
                                )
                                .with_range(*range),
                            );
                            Type::Error
                        }
                    };
                    let fn_type = Type::fn_(
                        varn_sem::types::FunctionType {
                            params: vec![varn_sem::types::FunctionParam {
                                name: Some(Arc::from(varn_core::TokenKind::This.as_str())),
                                ty: receiver_ty.0,
                                optional: false,
                                is_rest: false,
                            }],
                            return_type: ret_ty.0,
                            is_arrow: false,
                            type_params: ext_params.clone(),
                            type_param_defaults: Vec::new(),
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let mangled_atom = self.intern_local(&mangled);
                    let mut sym = Symbol::new(SymbolKind::Function, mangled_atom, range.start.line)
                        .with_type(fn_type);
                    sym.col = range.start.column;
                    sym.offset = range.start.offset;
                    sym.has_explicit_type = return_type.is_some();
                    self.define_outside(outer, mangled_atom, sym);
                    self.extensions
                        .getters
                        .entry(Arc::from(type_name.as_str()))
                        .or_default()
                        .insert(Arc::from(key_str.as_str()), Arc::from(mangled.as_str()));
                    self.bind_extension_function_scope(range.start.line, receiver_ty, &[], *body);
                }
                ExtensionMember::Setter {
                    key,
                    param,
                    body,
                    range,
                    ..
                } => {
                    let key_str = self.interner.resolve(*key).to_string();
                    let mangled = varn_core::extension_names::ExtensionKind::Setter
                        .mangled(&type_name, &key_str);
                    let param_ty = self.param_type(param, crate::ParamSite::Declared);
                    let fn_type = Type::fn_(
                        varn_sem::types::FunctionType {
                            params: vec![
                                varn_sem::types::FunctionParam {
                                    name: Some(Arc::from(varn_core::TokenKind::This.as_str())),
                                    ty: receiver_ty.0,
                                    optional: false,
                                    is_rest: false,
                                },
                                varn_sem::types::FunctionParam {
                                    name: Some(Arc::from(
                                        varn_sem::infer::pattern_to_string(
                                            &param.pattern,
                                            Some(&self.interner),
                                        )
                                        .as_str(),
                                    )),
                                    ty: param_ty.0,
                                    optional: param.is_optional,
                                    is_rest: param.is_rest,
                                },
                            ],
                            return_type: Type::Void.0,
                            is_arrow: false,
                            type_params: ext_params.clone(),
                            type_param_defaults: Vec::new(),
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let mangled_atom = self.intern_local(&mangled);
                    let mut sym = Symbol::new(SymbolKind::Function, mangled_atom, range.start.line)
                        .with_type(fn_type);
                    sym.col = range.start.column;
                    sym.offset = range.start.offset;
                    sym.has_explicit_type = true;
                    self.define_outside(outer, mangled_atom, sym);
                    self.extensions
                        .setters
                        .entry(Arc::from(type_name.as_str()))
                        .or_default()
                        .insert(Arc::from(key_str.as_str()), Arc::from(mangled.as_str()));
                    self.bind_extension_function_scope(
                        range.start.line,
                        receiver_ty,
                        std::slice::from_ref(param),
                        *body,
                    );
                }
            }
        }
    }

    fn bind_extension_function_scope(
        &mut self,
        line: u32,
        receiver_ty: Type,
        params: &[varn_core::ast::Param],
        body: varn_core::ast::StmtId,
    ) {
        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        let this_atom = self.intern_local(varn_core::TokenKind::This.as_str());
        let this_sym = Symbol::new(SymbolKind::Parameter, this_atom, line).with_type(receiver_ty);
        self.define(this_atom, this_sym);

        for p in params {
            let ty = self.param_binding_type(p, crate::ParamSite::Declared);
            self.bind_pattern(
                &p.pattern,
                SymbolKind::Parameter,
                line,
                None,
                Some(ty),
                p.type_ann.is_some(),
            );
            if let Some(def) = p.default {
                self.bind_expr(def);
            }
        }

        self.bind_stmt(body);
        self.current = saved;
    }
}
