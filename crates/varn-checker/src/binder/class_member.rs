use super::type_inference::pattern_lead_name;
use super::Binder;
use crate::binder::{ClassMemberInfo, ClassMemberKind};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{FunctionParam, FunctionType, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::ClassMember;
use varn_core::TypeKind;

impl<'r> Binder<'r> {
    pub(crate) fn collect_class_member(
        &mut self,
        member: &ClassMember,
        _class_name: &str,
        _methods: &mut FxHashMap<Arc<str>, Type>,
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
                            .map(|ann| self.resolve_type(ann))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest {
                            let is_array = matches!(self.ty_table.get(ty.0), TypeKind::Array(_));
                            if !is_array {
                                ty = Type::array(
                                    ty,
                                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                );
                            }
                        }
                        FunctionParam {
                            name: Some(Arc::from(pattern_lead_name(&p.pattern, &self.interner))),
                            ty: ty.0,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect();

                let fn_ty = Type::fn_(
                    FunctionType {
                        params: ps,
                        return_type: Type::Void.0,
                        is_arrow: false,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );

                let ctor_atom = self.intern_local("constructor");
                let mut sym =
                    Symbol::new(SymbolKind::Method, ctor_atom, range.start.line).with_type(fn_ty);
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = true;
                let symbol_id = self.arena.push(sym);

                members.push(ClassMemberInfo {
                    name: Arc::from("constructor"),
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
                    symbol_id: Some(symbol_id),
                    ..Default::default()
                });

                for p in params {
                    if p.modifiers.visibility.is_some() || p.modifiers.is_readonly {
                        let name_str = pattern_lead_name(&p.pattern, &self.interner).to_owned();
                        let key_rc: Arc<str> = Arc::from(name_str.as_str());
                        let key_atom = self.intern_local(&name_str);
                        let ty = p
                            .type_ann
                            .as_ref()
                            .map(|ann| self.resolve_type(ann))
                            .unwrap_or(Type::Dynamic);

                        let mut sym =
                            Symbol::new(SymbolKind::Property, key_atom, p.range.start.line)
                                .with_type(ty);
                        sym.col = p.range.start.column;
                        sym.offset = p.range.start.offset;
                        sym.has_explicit_type = p.type_ann.is_some();
                        let symbol_id = self.arena.push(sym);

                        members.push(ClassMemberInfo {
                            name: key_rc,
                            kind: ClassMemberKind::Property,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: p.range.start.line.saturating_sub(1),
                            col: p.range.start.column,
                            offset: p.range.start.offset,
                            ty,
                            members: Vec::new(),
                            visibility: p.modifiers.visibility,
                            is_abstract: false,
                            is_readonly: p.modifiers.is_readonly,
                            is_override: false,
                            symbol_id: Some(symbol_id),
                            ..Default::default()
                        });
                    }
                }
            }
            ClassMember::Property {
                key,
                type_ann,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Arc<str> = Arc::from(self.interner.resolve(*key));
                let ty = type_ann
                    .as_ref()
                    .map(|ann| self.resolve_type(ann))
                    .unwrap_or(Type::Dynamic);

                let mut sym =
                    Symbol::new(SymbolKind::Property, *key, range.start.line).with_type(ty);
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = type_ann.is_some();
                let symbol_id = self.arena.push(sym);

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
                    symbol_id: Some(symbol_id),
                    ..Default::default()
                });
            }
            ClassMember::Method {
                key,
                type_params,
                params,
                return_type,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Arc<str> = Arc::from(self.interner.resolve(*key));
                let declared_ret = return_type
                    .as_ref()
                    .map(|ann| self.resolve_type(ann))
                    .unwrap_or(Type::Void);
                let ret = crate::types::async_fn_return(
                    declared_ret,
                    modifiers.is_async,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    &self.interner,
                );

                let ps: Vec<FunctionParam> = params
                    .iter()
                    .map(|p| {
                        let mut ty = p
                            .type_ann
                            .as_ref()
                            .map(|ann| self.resolve_type(ann))
                            .unwrap_or(Type::Dynamic);
                        if p.is_rest {
                            let is_array = matches!(self.ty_table.get(ty.0), TypeKind::Array(_));
                            if !is_array {
                                ty = Type::array(
                                    ty,
                                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                );
                            }
                        }
                        FunctionParam {
                            name: Some(Arc::from(pattern_lead_name(&p.pattern, &self.interner))),
                            ty: ty.0,
                            optional: p.is_optional || p.default.is_some(),
                            is_rest: p.is_rest,
                        }
                    })
                    .collect();

                let fn_tps: Vec<Arc<str>> = type_params
                    .iter()
                    .map(|tp| Arc::from(self.interner.resolve(tp.name)))
                    .collect();

                let fn_ty = Type::fn_(
                    FunctionType {
                        params: ps,
                        return_type: ret.0,
                        is_arrow: false,
                        type_params: fn_tps,
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );

                let mut sym =
                    Symbol::new(SymbolKind::Method, *key, range.start.line).with_type(fn_ty);
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = return_type.is_some();
                sym.is_async = modifiers.is_async;
                sym.is_generator = modifiers.is_generator;
                let symbol_id = self.arena.push(sym);

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
                    symbol_id: Some(symbol_id),
                    ..Default::default()
                });
            }
            ClassMember::Getter {
                key,
                return_type,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Arc<str> = Arc::from(self.interner.resolve(*key));
                let ty = return_type
                    .as_ref()
                    .map(|ann| self.resolve_type(ann))
                    .unwrap_or(Type::Dynamic);

                let mut sym =
                    Symbol::new(SymbolKind::Property, *key, range.start.line).with_type(ty);
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = return_type.is_some();
                let symbol_id = self.arena.push(sym);

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
                    symbol_id: Some(symbol_id),
                    ..Default::default()
                });
            }
            ClassMember::Setter {
                key,
                param,
                modifiers,
                range,
                ..
            } => {
                let key_rc: Arc<str> = Arc::from(self.interner.resolve(*key));
                let ty = param
                    .type_ann
                    .as_ref()
                    .map(|ann| self.resolve_type(ann))
                    .unwrap_or(Type::Dynamic);

                let has_explicit = param.type_ann.is_some();

                let mut sym =
                    Symbol::new(SymbolKind::Property, *key, range.start.line).with_type(ty);
                sym.col = range.start.column;
                sym.offset = range.start.offset;
                sym.has_explicit_type = has_explicit;
                let symbol_id = self.arena.push(sym);

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
                    ty,
                    members: Vec::new(),
                    visibility: modifiers.visibility,
                    is_abstract: modifiers.is_abstract,
                    is_readonly: false,
                    is_override: false,
                    symbol_id: Some(symbol_id),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
}
