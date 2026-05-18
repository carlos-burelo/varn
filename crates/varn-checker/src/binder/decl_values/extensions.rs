use std::rc::Rc;
use varn_core::ast::{ExtensionDecl, ExtensionMember, Pattern};

use super::super::type_resolution::resolve_type_node;
use super::type_node_to_name;
use crate::scope::ScopeKind;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;

impl super::super::Binder {
    pub(crate) fn bind_extension(&mut self, e: &ExtensionDecl) {
        let type_name = type_node_to_name(&e.target);

        let receiver_ty = resolve_type_node(&e.target, Some(self));

        for member in &e.members {
            match member {
                ExtensionMember::Method(method) => {
                    let mangled = format!("__ext_{}_{}", type_name, method.id);
                    let mut param_types: Vec<crate::types::FunctionParam> =
                        vec![crate::types::FunctionParam {
                            name: Some(Rc::from("this")),
                            ty: receiver_ty.clone(),
                            optional: false,
                            is_rest: false,
                        }];
                    for p in &method.params {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, Some(self)))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(ty.0, varn_core::TypeKind::Array(_)) {
                            ty = Type::array(ty);
                        }
                        param_types.push(crate::types::FunctionParam {
                            name: Some(Rc::from(
                                super::super::type_inference::pattern_to_string(&p.pattern)
                                    .as_str(),
                            )),
                            ty,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        });
                    }
                    let ret_ty = method
                        .return_type
                        .as_ref()
                        .map(|rt| resolve_type_node(rt, Some(self)))
                        .unwrap_or(Type::Void);
                    let fn_type = Type::fn_(crate::types::FunctionType {
                        params: param_types,
                        return_type: Box::new(ret_ty),
                        is_arrow: false,
                        type_params: method
                            .type_params
                            .iter()
                            .map(|t| Rc::from(t.name.as_str()))
                            .collect(),
                    });
                    let line = method.range.start.line;
                    let mut sym =
                        Symbol::new(SymbolKind::Function, Rc::from(mangled.as_str()), line)
                            .with_type(fn_type);
                    sym.col = method.range.start.column;
                    sym.offset = method.range.start.offset;
                    sym.is_async = method.modifiers.is_async;
                    sym.is_generator = method.modifiers.is_generator;
                    self.define(mangled.clone(), sym);
                    self.extensions
                        .methods
                        .entry(Rc::from(type_name.as_str()))
                        .or_default()
                        .insert(method.id.clone(), Rc::from(mangled.as_str()));
                    self.bind_extension_function_scope(
                        line,
                        receiver_ty.clone(),
                        &method.params,
                        &method.body,
                    );
                }
                ExtensionMember::Getter {
                    key,
                    return_type,
                    body,
                    range,
                    ..
                } => {
                    let mangled = format!("__extget_{type_name}_{key}");
                    let ret_ty = return_type
                        .as_ref()
                        .map(|rt| resolve_type_node(rt, Some(self)))
                        .unwrap_or(Type::Dynamic);
                    let fn_type = Type::fn_(crate::types::FunctionType {
                        params: vec![crate::types::FunctionParam {
                            name: Some(Rc::from("this")),
                            ty: receiver_ty.clone(),
                            optional: false,
                            is_rest: false,
                        }],
                        return_type: Box::new(ret_ty),
                        is_arrow: false,
                        type_params: vec![],
                    });
                    let mut sym = Symbol::new(
                        SymbolKind::Function,
                        Rc::from(mangled.as_str()),
                        range.start.line,
                    )
                    .with_type(fn_type);
                    sym.col = range.start.column;
                    sym.offset = range.start.offset;
                    self.define(mangled.clone(), sym);
                    self.extensions
                        .getters
                        .entry(Rc::from(type_name.as_str()))
                        .or_default()
                        .insert(key.clone(), Rc::from(mangled.as_str()));
                    self.bind_extension_function_scope(
                        range.start.line,
                        receiver_ty.clone(),
                        &[],
                        body,
                    );
                }
                ExtensionMember::Setter {
                    key,
                    param,
                    body,
                    range,
                    ..
                } => {
                    let mangled = format!("__extset_{type_name}_{key}");
                    let param_ty = param
                        .type_ann
                        .as_ref()
                        .or(match &param.pattern {
                            Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                            _ => None,
                        })
                        .map(|ann| resolve_type_node(ann, Some(self)))
                        .unwrap_or(Type::Dynamic);
                    let fn_type = Type::fn_(crate::types::FunctionType {
                        params: vec![
                            crate::types::FunctionParam {
                                name: Some(Rc::from("this")),
                                ty: receiver_ty.clone(),
                                optional: false,
                                is_rest: false,
                            },
                            crate::types::FunctionParam {
                                name: Some(Rc::from(
                                    super::super::type_inference::pattern_to_string(&param.pattern)
                                        .as_str(),
                                )),
                                ty: param_ty.clone(),
                                optional: param.is_optional,
                                is_rest: param.is_rest,
                            },
                        ],
                        return_type: Box::new(Type::Void),
                        is_arrow: false,
                        type_params: vec![],
                    });
                    let mut sym = Symbol::new(
                        SymbolKind::Function,
                        Rc::from(mangled.as_str()),
                        range.start.line,
                    )
                    .with_type(fn_type);
                    sym.col = range.start.column;
                    sym.offset = range.start.offset;
                    self.define(mangled.clone(), sym);
                    self.extensions
                        .setters
                        .entry(Rc::from(type_name.as_str()))
                        .or_default()
                        .insert(key.clone(), Rc::from(mangled.as_str()));
                    self.bind_extension_function_scope(
                        range.start.line,
                        receiver_ty.clone(),
                        std::slice::from_ref(param),
                        body,
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
        body: &varn_core::ast::Stmt,
    ) {
        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        let this_sym =
            Symbol::new(SymbolKind::Parameter, Rc::from("this"), line).with_type(receiver_ty);
        self.define("this".to_owned(), this_sym);

        for p in params {
            let mut ty = p
                .type_ann
                .as_ref()
                .or(match &p.pattern {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|ann| resolve_type_node(ann, Some(self)))
                .unwrap_or(Type::Dynamic);
            if p.is_rest && !matches!(ty.0, varn_core::TypeKind::Array(_)) {
                ty = Type::array(ty);
            }
            self.bind_pattern(&p.pattern, SymbolKind::Parameter, line, None, Some(ty));
            if let Some(def) = &p.default {
                self.bind_expr(def);
            }
        }

        self.bind_stmt(body);
        self.current = saved;
    }
}
