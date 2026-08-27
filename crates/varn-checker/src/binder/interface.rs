use super::type_inference::pattern_lead_name;
use super::type_resolution::resolve_type_node;
use crate::binder::{ClassMemberInfo, ClassMemberKind};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{FunctionParam, FunctionType, Type};
use std::rc::Rc;
use varn_core::ast::{InterfaceDecl, InterfaceMember, Pattern};
use varn_core::TypeKind;

impl<'r> super::Binder<'r> {
    pub(super) fn bind_interface(&mut self, i: &InterfaceDecl) {
        let name_rc: Rc<str> = Rc::from(i.id.as_ref());
        let origin_rc: Option<Rc<str>> = if self.source_file.is_empty() {
            None
        } else {
            Some(Rc::from(self.source_file.as_ref()))
        };
        let mut sym = Symbol::new(SymbolKind::Interface, i.id.clone(), i.range.start.line);
        sym.ty = Some(crate::types::Type(
            varn_core::TypeKind::Named(name_rc, origin_rc),
            false,
        ));
        sym.col = i.range.start.column;
        sym.offset = i.range.start.offset;
        sym.doc = i.doc.as_ref().map(|s| Rc::from(s.as_str()));
        sym.type_params = i
            .type_params
            .iter()
            .map(|t| Rc::from(t.name.as_str()))
            .collect();
        sym.type_param_constraints = i
            .type_params
            .iter()
            .map(|t| {
                t.constraint
                    .as_ref()
                    .map(|con| resolve_type_node(con, Some(self)))
            })
            .collect();
        self.define(i.id.to_string(), sym);

        let child = self
            .scopes
            .child(crate::scope::ScopeKind::Interface, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&i.type_params, i.range.start.line);

        let mut members: Vec<ClassMemberInfo> = Vec::new();
        for member in &i.body {
            self.collect_interface_member(member, &mut members);
        }

        if !members.is_empty() {
            self.type_members
                .interfaces
                .insert(i.id.clone(), members.clone());
            self.type_members.flattened.insert(i.id.clone(), members);
        }

        self.current = saved;
    }

    fn collect_interface_member(
        &mut self,
        member: &InterfaceMember,
        members: &mut Vec<ClassMemberInfo>,
    ) {
        match member {
            InterfaceMember::Property {
                key,
                type_ann,
                optional,
                range,
                ..
            } => {
                let ty = resolve_type_node(type_ann, Some(self));

                let mut sym = Symbol::new(SymbolKind::Property, key.clone(), range.start.line)
                    .with_type(ty.clone());
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = true;
                let symbol_id = self.arena.push(sym);

                members.push(ClassMemberInfo {
                    name: key.clone(),
                    kind: ClassMemberKind::Property,
                    is_async: false,
                    is_generator: false,
                    is_static: false,
                    is_optional: *optional,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty,
                    members: Vec::new(),
                    visibility: None,
                    is_abstract: false,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: Some(symbol_id),
                                ..Default::default()
                                });
            }
            InterfaceMember::Method {
                key,
                type_params,
                params,
                return_type,
                optional,
                is_async,
                range,
            } => {
                let ret = crate::types::async_fn_return(
                    return_type
                        .as_ref()
                        .map(|m| resolve_type_node(m, Some(self)))
                        .unwrap_or(Type::Dynamic),
                    *is_async,
                );
                let params_list = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                                _ => None,
                            })
                            .map(|m| resolve_type_node(m, Some(self)))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                            ty = Type::array(ty);
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                            ty,
                            optional: p.is_optional,
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();

                let fn_tps: Vec<Rc<str>> = type_params
                    .iter()
                    .map(|tp| Rc::from(tp.name.as_str()))
                    .collect();

                let fn_type = Type::fn_(FunctionType {
                    params: params_list,
                    return_type: Box::new(ret),
                    is_arrow: false,
                    type_params: fn_tps,
                });

                let mut sym = Symbol::new(SymbolKind::Method, key.clone(), range.start.line)
                    .with_type(fn_type.clone());
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = return_type.is_some();
                let symbol_id = self.arena.push(sym);

                members.push(ClassMemberInfo {
                    name: key.clone(),
                    kind: ClassMemberKind::Method,
                    is_async: *is_async,
                    is_generator: false,
                    is_static: false,
                    is_optional: *optional,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: fn_type,
                    members: Vec::new(),
                    visibility: None,
                    is_abstract: false,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: Some(symbol_id),
                                ..Default::default()
                                });
            }
            InterfaceMember::Index {
                param,
                return_type,
                range,
                ..
            } => {
                let ret = resolve_type_node(return_type, Some(self));
                let param_name = pattern_lead_name(&param.pattern);
                let key_ty = param
                    .type_ann
                    .as_ref()
                    .map(|m| resolve_type_node(m, Some(self)))
                    .unwrap_or(Type::Dynamic);
                members.push(ClassMemberInfo {
                    name: Rc::from(format!("[{param_name}: {key_ty}]")),
                    kind: ClassMemberKind::Property,
                    is_async: false,
                    is_generator: false,
                    is_static: false,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: ret,
                    members: Vec::new(),
                    visibility: None,
                    is_abstract: false,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: None,
                    ..Default::default()
                });
            }
            InterfaceMember::Callable {
                params,
                return_type,
                range,
            } => {
                let ret = resolve_type_node(return_type, Some(self));
                let params_list = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .or(match &p.pattern {
                                Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                                _ => None,
                            })
                            .map(|m| resolve_type_node(m, Some(self)))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest && !matches!(ty.0, TypeKind::Array(_)) {
                            ty = Type::array(ty);
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                            ty,
                            optional: p.is_optional,
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();
                let fn_type = Type::fn_(FunctionType {
                    params: params_list,
                    return_type: Box::new(ret),
                    is_arrow: false,
                    type_params: vec![],
                });
                members.push(ClassMemberInfo {
                    name: Rc::from(varn_core::MemberKey::Callable.as_str()),
                    kind: ClassMemberKind::Method,
                    is_async: false,
                    is_generator: false,
                    is_static: false,
                    is_optional: false,
                    line: range.start.line.saturating_sub(1),
                    col: range.start.column,
                    offset: range.start.offset,
                    ty: fn_type,
                    members: Vec::new(),
                    visibility: None,
                    is_abstract: false,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: None,
                    ..Default::default()
                });
            }
        }
    }
}
