use std::sync::Arc;
use varn_core::ast::{NamespaceDecl, StructDecl};

use crate::binder::{pattern_lead_name, ClassMemberInfo, ClassMemberKind};
use crate::scope::ScopeKind;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_namespace(&mut self, n: &NamespaceDecl) {
        let id_rc: Arc<str> = Arc::from(self.interner.resolve(n.id));
        let namespace_ty = Type::named_with_origin(
            id_rc.clone(),
            Some(Arc::from(self.source_file.as_ref())),
            self.resolver,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let mut sym =
            Symbol::new(SymbolKind::Namespace, n.id, n.range.start.line).with_type(namespace_ty);
        sym.doc = n.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        self.define(n.id, sym);

        let child = self.scopes.child(ScopeKind::Namespace, self.current);
        let saved = self.current;
        self.current = child;

        for d in &n.body {
            self.bind_decl(d);
        }

        let members = self.collect_namespace_members(&n.body);
        if !members.is_empty() {
            self.type_members.namespaces.insert(id_rc, members);
        }

        self.current = saved;
    }

    fn collect_namespace_members(&mut self, body: &[varn_core::ast::Decl]) -> Vec<ClassMemberInfo> {
        use varn_core::ast::Decl;
        let mut members = Vec::new();
        for decl in body {
            match decl {
                Decl::Function(f) => {
                    let declared = f
                        .return_type
                        .as_ref()
                        .map(|m| self.resolve_type(m))
                        .unwrap_or(Type::Void);
                    let ret = crate::types::async_fn_return(
                        declared,
                        f.modifiers.is_async,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        &self.interner,
                        Some(self.resolver),
                    );
                    let params_list = f
                        .params
                        .iter()
                        .map(|p| {
                            let mut ty = p
                                .type_ann
                                .as_ref()
                                .or(match &p.pattern {
                                    varn_core::ast::Pattern::Identifier { type_ann, .. } => {
                                        type_ann.as_ref()
                                    }
                                    _ => None,
                                })
                                .map(|ann| self.resolve_type(ann))
                                .unwrap_or(Type::Dynamic);
                            if p.is_rest {
                                let is_array = matches!(
                                    self.ty_table.get(ty.0),
                                    varn_core::TypeKind::Array(_)
                                );
                                if !is_array {
                                    ty = Type::array(
                                        ty,
                                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                    );
                                }
                            }
                            crate::types::FunctionParam {
                                name: Some(Arc::from(pattern_lead_name(
                                    &p.pattern,
                                    &self.interner,
                                ))),
                                ty: ty.0,
                                optional: p.is_optional || p.default.is_some(),
                                is_rest: p.is_rest,
                            }
                        })
                        .collect::<Vec<_>>();
                    let fn_type = Type::fn_(
                        crate::types::FunctionType {
                            params: params_list,
                            return_type: ret.0,
                            is_arrow: false,
                            type_params: f
                                .type_params
                                .iter()
                                .map(|t| Arc::from(self.interner.resolve(t.name)))
                                .collect(),
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(f.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: Arc::from(self.interner.resolve(f.id)),
                        kind: ClassMemberKind::Function,
                        is_async: f.modifiers.is_async,
                        is_generator: f.modifiers.is_generator,
                        is_static: false,
                        is_optional: false,
                        line: f.range.start.line.saturating_sub(1),
                        col: f.range.start.column,
                        offset: f.range.start.offset,
                        ty: fn_type,
                        members: Vec::new(),
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Class(c) => {
                    let name: Arc<str> =
                        c.id.map(|a| Arc::from(self.interner.resolve(a)))
                            .unwrap_or_else(|| Arc::from(""));
                    let class_members = self
                        .type_members
                        .classes
                        .get(&name)
                        .map(|e| e.members.clone())
                        .unwrap_or_default();
                    let scope = self.scopes.get(self.current);
                    let symbol_id = c.id.and_then(|a| scope.resolve(a, &self.scopes));
                    let class_ty = Type::named_with_origin(
                        name.clone(),
                        Some(Arc::from(self.source_file.as_ref())),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name,
                        kind: ClassMemberKind::Class,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: c.range.start.line.saturating_sub(1),
                        col: c.range.start.column,
                        offset: c.range.start.offset,
                        ty: class_ty,
                        members: class_members,
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        let name: Arc<str> = Arc::from(pattern_lead_name(&d.id, &self.interner));
                        let ty = d
                            .type_ann
                            .as_ref()
                            .map(|ann| self.resolve_type(ann))
                            .or_else(|| {
                                d.init
                                    .as_ref()
                                    .map(|e| self.infer_expr_type_self(*e))
                                    .filter(|t| !t.is_dynamic())
                            })
                            .unwrap_or(Type::Dynamic);
                        let scope = self.scopes.get(self.current);
                        let symbol_id = self
                            .interner
                            .get(&name)
                            .and_then(|a| scope.resolve(a, &self.scopes));
                        members.push(ClassMemberInfo {
                            name,
                            kind: ClassMemberKind::Variable,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: d.range.start.line.saturating_sub(1),
                            col: d.range.start.column,
                            offset: d.range.start.offset,
                            ty,
                            members: Vec::new(),
                            visibility: None,
                            is_abstract: false,
                            is_readonly: false,
                            is_override: false,
                            symbol_id,
                            ..Default::default()
                        });
                    }
                }
                Decl::Namespace(n) => {
                    let inner_members = self.collect_namespace_members(&n.body);
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(n.id, &self.scopes);
                    let n_id_rc: Arc<str> = Arc::from(self.interner.resolve(n.id));
                    let ns_ty = Type::named(
                        n_id_rc.clone(),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name: n_id_rc,
                        kind: ClassMemberKind::Namespace,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: n.range.start.line.saturating_sub(1),
                        col: n.range.start.column,
                        offset: n.range.start.offset,
                        ty: ns_ty,
                        members: inner_members,
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::TypeAlias(t) => {
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(t.id, &self.scopes);
                    let t_id_rc: Arc<str> = Arc::from(self.interner.resolve(t.id));
                    let alias_ty = Type::named_with_origin(
                        t_id_rc.clone(),
                        Some(Arc::from(self.source_file.as_ref())),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name: t_id_rc,
                        kind: ClassMemberKind::Property,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: t.range.start.line.saturating_sub(1),
                        col: t.range.start.column,
                        offset: t.range.start.offset,
                        ty: alias_ty,
                        members: Vec::new(),
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Enum(e) => {
                    let e_id_rc: Arc<str> = Arc::from(self.interner.resolve(e.id));
                    let variants = self
                        .type_members
                        .enums
                        .get(&e_id_rc)
                        .cloned()
                        .unwrap_or_default();
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(e.id, &self.scopes);
                    let enum_ty = Type::named(
                        e_id_rc.clone(),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name: e_id_rc,
                        kind: ClassMemberKind::Enum,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: e.range.start.line.saturating_sub(1),
                        col: e.range.start.column,
                        offset: e.range.start.offset,
                        ty: enum_ty,
                        members: variants,
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Struct(s) => {
                    let struct_members = self
                        .type_members
                        .objects
                        .get(&s.id)
                        .cloned()
                        .unwrap_or_default();
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(s.id, &self.scopes);
                    let s_id_rc: Arc<str> = Arc::from(self.interner.resolve(s.id));
                    let struct_ty = Type::named(
                        s_id_rc.clone(),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name: s_id_rc,
                        kind: ClassMemberKind::Struct,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: s.range.start.line.saturating_sub(1),
                        col: s.range.start.column,
                        offset: s.range.start.offset,
                        ty: struct_ty,
                        members: struct_members,
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Interface(i) => {
                    let i_id_rc: Arc<str> = Arc::from(self.interner.resolve(i.id));
                    let interface_members = self
                        .type_members
                        .interfaces
                        .get(&i_id_rc)
                        .cloned()
                        .unwrap_or_default();
                    let scope = self.scopes.get(self.current);
                    let symbol_id = scope.resolve(i.id, &self.scopes);
                    let iface_ty = Type::named(
                        i_id_rc.clone(),
                        self.resolver,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    members.push(ClassMemberInfo {
                        name: i_id_rc,
                        kind: ClassMemberKind::Interface,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: i.range.start.line.saturating_sub(1),
                        col: i.range.start.column,
                        offset: i.range.start.offset,
                        ty: iface_ty,
                        members: interface_members,
                        visibility: None,
                        is_abstract: false,
                        is_readonly: false,
                        is_override: false,
                        symbol_id,
                        ..Default::default()
                    });
                }
                Decl::Export(e) => {
                    use varn_core::ast::ExportDecl;
                    if let ExportDecl::Decl { declaration, .. } = e {
                        let inner =
                            self.collect_namespace_members(std::slice::from_ref(declaration));
                        members.extend(inner);
                    }
                }
                _ => {}
            }
        }
        members
    }

    pub(crate) fn bind_struct(&mut self, s: &StructDecl) {
        let id_rc: Arc<str> = Arc::from(self.interner.resolve(s.id));
        let struct_ty = Type::named_with_origin(
            id_rc.clone(),
            Some(Arc::from(self.source_file.as_ref())),
            self.resolver,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let mut sym =
            Symbol::new(SymbolKind::Struct, s.id, s.range.start.line).with_type(struct_ty);
        sym.doc = s.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        self.define(s.id, sym);

        let mut members = Vec::new();
        for field in &s.fields {
            let ty = self.resolve_type(&field.type_ann);
            let field_name_rc: Arc<str> = Arc::from(self.interner.resolve(field.name));

            let mut field_sym =
                Symbol::new(SymbolKind::Property, field.name, field.range.start.line)
                    .with_type(ty.clone());
            field_sym.col = field.range.start.column;
            field_sym.offset = field.range.start.offset;
            field_sym.has_explicit_type = true;
            let symbol_id = self.arena.push(field_sym);

            members.push(ClassMemberInfo {
                name: field_name_rc,
                kind: ClassMemberKind::Property,
                is_async: false,
                is_generator: false,
                is_static: false,
                is_optional: false,
                line: field.range.start.line.saturating_sub(1),
                col: field.range.start.column,
                offset: field.range.start.offset,
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
        let struct_info = ClassMemberInfo {
            name: id_rc.clone(),
            kind: ClassMemberKind::Struct,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: s.range.start.line.saturating_sub(1),
            col: s.range.start.column,
            offset: s.range.start.offset,
            ty: Type::named_with_origin(
                id_rc.clone(),
                Some(Arc::from(self.source_file.as_ref())),
                self.resolver,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            members,
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
            ..Default::default()
        };
        self.type_members.classes.insert(id_rc, struct_info);
    }
}
