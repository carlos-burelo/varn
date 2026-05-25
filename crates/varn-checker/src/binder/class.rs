use super::type_inference::pattern_lead_name;
use super::type_resolution::resolve_type_node;
use crate::binder::{ClassMemberInfo, ClassMemberKind, PendingEnrich};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{FunctionParam, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{ClassDecl, ClassMember, Pattern, Stmt};
use varn_core::{IntrinsicType, TypeKind};

impl super::Binder {
    pub(super) fn bind_class(&mut self, c: &ClassDecl) {
        let name: Rc<str> =
            c.id.as_ref()
                .map(|s| Rc::from(s.as_ref()))
                .unwrap_or_else(|| Rc::from("<anon>"));
        let line = c.range.start.line;
        let cls_type =
            Type::named_with_origin(name.clone(), Some(Rc::from(self.source_file.as_ref())));
        let mut sym =
            Symbol::new(SymbolKind::Class, name.clone(), line).with_type(cls_type.clone());
        sym.col = c.range.start.column;
        sym.offset = c.range.start.offset;
        sym.doc = c.doc.as_ref().map(|s| Rc::from(s.as_str()));
        sym.type_params = c
            .type_params
            .iter()
            .map(|t| Rc::from(t.name.as_str()))
            .collect();
        sym.type_param_constraints = c
            .type_params
            .iter()
            .map(|t| {
                t.constraint
                    .as_ref()
                    .map(|con| resolve_type_node(con, Some(self)))
            })
            .collect();
        self.define(name.to_string(), sym);

        let child = self
            .scopes
            .child(crate::scope::ScopeKind::Class, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&c.type_params, line);

        let mut methods: FxHashMap<Rc<str>, Type> = FxHashMap::default();
        let mut members: Vec<ClassMemberInfo> = Vec::new();

        for member in &c.body {
            self.collect_class_member(member, name.as_ref(), &mut methods, &mut members);
        }

        for member in &c.body {
            match member {
                ClassMember::Constructor {
                    params,
                    body,
                    range,
                    ..
                } => {
                    self.bind_inline_function(&[], params, None, body, range);
                }
                ClassMember::Method {
                    key,
                    type_params,
                    params,
                    return_type,
                    body: Some(body),
                    range,
                    modifiers,
                    ..
                } => {
                    let key_rc: Rc<str> = Rc::from(key.as_ref());
                    if return_type.is_none() && !modifiers.is_abstract {
                        self.pending_enrich.push(PendingEnrich::Method {
                            class_name: name.clone(),
                            key: key_rc,
                            body: body as *const Stmt,
                            is_async: modifiers.is_async,
                        });
                    }
                    self.bind_inline_function(
                        type_params,
                        params,
                        return_type.as_ref(),
                        body,
                        range,
                    );
                }
                ClassMember::Getter {
                    key,
                    return_type,
                    body: Some(body),
                    range: _,
                    ..
                } => {
                    let key_rc: Rc<str> = Rc::from(key.as_ref());
                    if return_type.is_none() {
                        self.pending_enrich.push(PendingEnrich::Getter {
                            class_name: name.clone(),
                            key: key_rc,
                            body: body as *const Stmt,
                        });
                    }
                    self.bind_stmt(body);
                }
                ClassMember::Setter {
                    key,
                    param,
                    body: Some(body),
                    range,
                    ..
                } => {
                    let key_rc: Rc<str> = Rc::from(key.as_ref());
                    self.pending_enrich.push(PendingEnrich::Setter {
                        class_name: name.clone(),
                        key: key_rc,
                        body: body as *const Stmt,
                    });
                    self.bind_stmt(body);
                    self.bind_pattern(
                        &param.pattern,
                        SymbolKind::Parameter,
                        range.start.line,
                        None,
                        None,
                    );
                }
                ClassMember::Property {
                    init: Some(init), ..
                } => {
                    self.bind_expr(init);
                }
                _ => {}
            }
        }

        let extends = c.super_class.as_ref().and_then(|e| {
            match super::type_inference::infer_expr_type(e, Some(self)).0 {
                TypeKind::Named(n, o) => Some((n, o)),
                TypeKind::Generic(n, _, o) => Some((n, o)),
                _ => None,
            }
        });

        let mut final_members = members.clone();
        if let Some((parent_name, parent_origin)) = extends {
            self.class_parents
                .insert(name.clone(), Rc::from(parent_name.as_ref()));
            if let Some(parent_members) =
                self.get_class_members(parent_name.as_ref(), parent_origin.as_deref())
            {
                for pm in parent_members {
                    if !members.iter().any(|m| m.name == pm.name) {
                        final_members.push(pm.clone());
                    }
                }
            }
        }

        let class_info = ClassMemberInfo {
            name: name.clone(),
            kind: ClassMemberKind::Class,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: c.range.start.line.saturating_sub(1),
            col: c.range.start.column,
            offset: c.range.start.offset,
            ty: cls_type,
            members: final_members,
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
        };

        self.type_members.classes.insert(name, class_info);
        self.current = saved;
    }

    pub(crate) fn collect_class_member(
        &self,
        member: &ClassMember,
        _class_name: &str,
        _methods: &mut FxHashMap<Rc<str>, Type>,
        members: &mut Vec<ClassMemberInfo>,
    ) {
        match member {
            ClassMember::Constructor { params, range, .. } => {
                let ps: Vec<FunctionParam> = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, Some(self)))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                            ty = Type::array(ty);
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                            ty,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect();

                let fn_ty = Type::fn_(FunctionType {
                    params: ps,
                    return_type: Box::new(Type::Void),
                    is_arrow: false,
                    type_params: vec![],
                });

                members.push(ClassMemberInfo {
                    name: Rc::from("constructor"),
                    kind: ClassMemberKind::Constructor,
                    is_async: false,
                    is_generator: false,
                    is_static: false,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: fn_ty,
                    members: Vec::new(),
                    visibility: None,
                    is_abstract: false,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: None,
                });
            }
            ClassMember::Property {
                key,
                type_ann,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Rc<str> = Rc::from(key.as_ref());
                let ty = type_ann
                    .as_ref()
                    .map(|ann| resolve_type_node(ann, Some(self)))
                    .unwrap_or(Type::Dynamic);

                members.push(ClassMemberInfo {
                    name: key_rc,
                    kind: ClassMemberKind::Property,
                    is_async: false,
                    is_generator: false,
                    is_static: modifiers.is_static,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty,
                    members: Vec::new(),
                    visibility: modifiers.visibility,
                    is_abstract: false,
                    is_readonly: modifiers.is_readonly,
                    is_override: modifiers.is_override,
                    symbol_id: None,
                });
            }
            ClassMember::Method {
                key,
                params,
                return_type,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Rc<str> = Rc::from(key.as_ref());
                let ret = return_type
                    .as_ref()
                    .map(|ann| resolve_type_node(ann, Some(self)))
                    .unwrap_or(if modifiers.is_async {
                        Type::generic_with_origin(
                            Rc::from(IntrinsicType::Task.as_str()),
                            vec![Type::Void],
                            Some(Rc::from("std/async")),
                        )
                    } else {
                        Type::Void
                    });

                let ps: Vec<FunctionParam> = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                                _ => None,
                            })
                            .map(|ann| resolve_type_node(ann, Some(self)))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                            ty = Type::array(ty);
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                            ty,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect();

                let fn_ty = Type::fn_(FunctionType {
                    params: ps,
                    return_type: Box::new(ret),
                    is_arrow: false,
                    type_params: vec![],
                });

                members.push(ClassMemberInfo {
                    name: key_rc,
                    kind: ClassMemberKind::Method,
                    is_async: modifiers.is_async,
                    is_generator: modifiers.is_generator,
                    is_static: modifiers.is_static,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: fn_ty,
                    members: Vec::new(),
                    visibility: modifiers.visibility,
                    is_abstract: modifiers.is_abstract,
                    is_readonly: false,
                    is_override: modifiers.is_override,
                    symbol_id: None,
                });
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Rc<str> = Rc::from(key.as_ref());
                let ty = return_type
                    .as_ref()
                    .map(|ann| resolve_type_node(ann, Some(self)))
                    .unwrap_or(Type::Dynamic);
                members.push(ClassMemberInfo {
                    name: key_rc,
                    kind: ClassMemberKind::Getter,
                    is_async: false,
                    is_generator: false,
                    is_static: modifiers.is_static,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty,
                    members: Vec::new(),
                    visibility: modifiers.visibility,
                    is_abstract: modifiers.is_abstract,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: None,
                });
            }
            ClassMember::Setter {
                key,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Rc<str> = Rc::from(key.as_ref());
                members.push(ClassMemberInfo {
                    name: key_rc,
                    kind: ClassMemberKind::Setter,
                    is_async: false,
                    is_generator: false,
                    is_static: modifiers.is_static,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: Type::Dynamic,
                    members: Vec::new(),
                    visibility: modifiers.visibility,
                    is_abstract: modifiers.is_abstract,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: None,
                });
            }
            _ => {}
        }
    }

    pub(crate) fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<&Vec<ClassMemberInfo>> {
        if let Some(res) = self.type_members.classes.get(name) {
            return Some(&res.members);
        }
        if let Some(origin) = origin {
            let mut visiting = Vec::new();
            let exports = crate::module_resolver::resolve_module_exports(origin, &mut visiting);
            if let Some(sym) = exports.get(name) {
                if sym.kind == SymbolKind::Class {
                    return self.type_members.classes.get(name).map(|e| &e.members);
                }
            }
        }
        None
    }

    pub(crate) fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<&Vec<ClassMemberInfo>> {
        if let Some(res) = self.type_members.interfaces.get(name) {
            return Some(res);
        }
        if let Some(origin) = origin {
            let mut visiting = Vec::new();
            let exports = crate::module_resolver::resolve_module_exports(origin, &mut visiting);
            if let Some(sym) = exports.get(name) {
                if sym.kind == SymbolKind::Interface {
                    return self.type_members.interfaces.get(name);
                }
            }
        }
        None
    }
}
