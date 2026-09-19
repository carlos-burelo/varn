use super::type_inference::pattern_lead_name;
use crate::binder::{ClassMemberInfo, ClassMemberKind};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{FunctionParam, FunctionType, Type};
use std::rc::Rc;
use varn_core::ast::{InterfaceDecl, InterfaceMember, Pattern};
use varn_core::TypeKind;

impl<'r> super::Binder<'r> {
    pub(super) fn bind_interface(&mut self, i: &InterfaceDecl) {
        let id_rc: Rc<str> = Rc::from(self.interner.resolve(i.id));
        let origin_rc: Option<Rc<str>> = if self.source_file.is_empty() {
            None
        } else {
            Some(Rc::from(self.source_file.as_ref()))
        };
        let mut sym = Symbol::new(SymbolKind::Interface, i.id, i.range.start.line);
        sym.ty = Some(Type::named_with_origin(
            id_rc.clone(),
            origin_rc,
            self.resolver,
            &mut self.ty_table,
        ));
        sym.col = i.range.start.column;
        sym.offset = i.range.start.offset;
        sym.doc = i.doc.as_ref().map(|s| self.interner.intern(s.as_str()));
        sym.type_params = i.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = i
            .type_params
            .iter()
            .map(|t| {
                t.constraint
                    .as_ref()
                    .map(|con| self.resolve_type(con))
            })
            .collect();
        self.define(i.id, sym);

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
                .insert(id_rc.clone(), members.clone());
            self.type_members.flattened.insert(id_rc, members);
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
                let ty = self.resolve_type(type_ann);
                let key_rc: Rc<str> = Rc::from(self.interner.resolve(*key));

                let mut sym = Symbol::new(SymbolKind::Property, *key, range.start.line)
                    .with_type(ty.clone());
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = true;
                let symbol_id = self.arena.push(sym);

                members.push(ClassMemberInfo {
                    name: key_rc,
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
                let declared = return_type
                    .as_ref()
                    .map(|m| self.resolve_type(m))
                    .unwrap_or(Type::Dynamic);
                let ret = crate::types::async_fn_return(
                    declared,
                    *is_async,
                    &mut self.ty_table,
                    &self.interner,
                    Some(self.resolver),
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
                            .map(|m| self.resolve_type(m))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest {
                            let is_array = matches!(self.ty_table.get(ty.0), TypeKind::Array(_));
                            if !is_array {
                                ty = Type::array(ty, &mut self.ty_table);
                            }
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern, &self.interner))),
                            ty: ty.0,
                            optional: p.is_optional,
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();

                let fn_tps: Vec<Rc<str>> = type_params
                    .iter()
                    .map(|tp| Rc::from(self.interner.resolve(tp.name)))
                    .collect();

                let fn_type = Type::fn_(
                    FunctionType {
                        params: params_list,
                        return_type: ret.0,
                        is_arrow: false,
                        type_params: fn_tps,
                    },
                    &mut self.ty_table,
                );

                let key_rc: Rc<str> = Rc::from(self.interner.resolve(*key));
                let mut sym = Symbol::new(SymbolKind::Method, *key, range.start.line)
                    .with_type(fn_type.clone());
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = return_type.is_some();
                let symbol_id = self.arena.push(sym);

                members.push(ClassMemberInfo {
                    name: key_rc,
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
                let ret = self.resolve_type(return_type);
                let param_name = pattern_lead_name(&param.pattern, &self.interner).to_string();
                let key_ty = param
                    .type_ann
                    .as_ref()
                    .map(|m| self.resolve_type(m))
                    .unwrap_or(Type::Dynamic);
                members.push(ClassMemberInfo {
                    name: Rc::from(format!(
                        "[{param_name}: {}]",
                        key_ty.display(&self.ty_table, &self.interner)
                    )),
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
                let ret = self.resolve_type(return_type);
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
                            .map(|m| self.resolve_type(m))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest {
                            let is_array = matches!(self.ty_table.get(ty.0), TypeKind::Array(_));
                            if !is_array {
                                ty = Type::array(ty, &mut self.ty_table);
                            }
                        }
                        FunctionParam {
                            name: Some(Rc::from(pattern_lead_name(&p.pattern, &self.interner))),
                            ty: ty.0,
                            optional: p.is_optional,
                            is_rest: p.is_rest,
                        }
                    })
                    .collect::<Vec<_>>();
                let fn_type = Type::fn_(
                    FunctionType {
                        params: params_list,
                        return_type: ret.0,
                        is_arrow: false,
                        type_params: vec![],
                    },
                    &mut self.ty_table,
                );
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
