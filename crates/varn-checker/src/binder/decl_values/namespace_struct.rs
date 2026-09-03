use std::rc::Rc;
use varn_core::ast::{NamespaceDecl, StructDecl};

use super::super::type_resolution::resolve_type_node;
use crate::binder::{infer_expr_type, pattern_lead_name, ClassMemberInfo, ClassMemberKind};
use crate::scope::ScopeKind;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_namespace(&mut self, n: &NamespaceDecl) {
        let mut sym =
            Symbol::new(SymbolKind::Namespace, n.id.clone(), n.range.start.line).with_type(
                Type::named_with_origin(n.id.clone(), Some(Rc::from(self.source_file.as_ref()))),
            );
        sym.doc = n.doc.as_ref().map(|s| Rc::from(s.as_str()));
        self.define(n.id.to_string(), sym);

        let child = self.scopes.child(ScopeKind::Namespace, self.current);
        let saved = self.current;
        self.current = child;

        for d in &n.body {
            self.bind_decl(d);
        }

        let members = self.collect_namespace_members(&n.body);
        if !members.is_empty() {
            self.type_members.namespaces.insert(n.id.clone(), members);
        }

        self.current = saved;
    }

    fn collect_namespace_members(&self, body: &[varn_core::ast::Decl]) -> Vec<ClassMemberInfo> {
        use varn_core::ast::Decl;
        let mut members = Vec::new();
        let scope = self.scopes.get(self.current);
        for decl in body {
            match decl {
                Decl::Function(f) => {
                    let ret = crate::types::async_fn_return(
                        f.return_type
                            .as_ref()
                            .map(|m| resolve_type_node(m, Some(self)))
                            .unwrap_or(Type::Void),
                        f.modifiers.is_async,
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
                                .map(|ann| resolve_type_node(ann, Some(self)))
                                .unwrap_or(Type::Dynamic);
                            if p.is_rest && !matches!(ty.0, varn_core::TypeKind::Array(_)) {
                                ty = Type::array(ty);
                            }
                            crate::types::FunctionParam {
                                name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                                ty,
                                optional: p.is_optional || p.default.is_some(),
                                is_rest: p.is_rest,
                            }
                        })
                        .collect::<Vec<_>>();
                    let fn_type = Type::fn_(crate::types::FunctionType {
                        params: params_list,
                        return_type: Box::new(ret.clone()),
                        is_arrow: false,
                        type_params: f
                            .type_params
                            .iter()
                            .map(|t| Rc::from(t.name.as_str()))
                            .collect(),
                    });
                    let symbol_id = scope.resolve(&f.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: f.id.clone(),
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
                    let name = c.id.clone().unwrap_or_else(|| Rc::from(""));
                    let class_members = self
                        .type_members
                        .classes
                        .get(&name)
                        .map(|e| e.members.clone())
                        .unwrap_or_default();
                    let symbol_id = scope.resolve(&name, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: name.clone(),
                        kind: ClassMemberKind::Class,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: c.range.start.line.saturating_sub(1),
                        col: c.range.start.column,
                        offset: c.range.start.offset,
                        ty: Type::named_with_origin(
                            name,
                            Some(Rc::from(self.source_file.as_ref())),
                        ),
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
                        let name = Rc::from(pattern_lead_name(&d.id));
                        let ty = d
                            .type_ann
                            .as_ref()
                            .map(|ann| resolve_type_node(ann, Some(self)))
                            .or_else(|| {
                                d.init
                                    .as_ref()
                                    .map(|e| infer_expr_type(e, Some(self)))
                                    .filter(|t| !t.is_dynamic())
                            })
                            .unwrap_or(Type::Dynamic);
                        let symbol_id = scope.resolve(&name, &self.scopes);
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
                    let symbol_id = scope.resolve(&n.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: n.id.clone(),
                        kind: ClassMemberKind::Namespace,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: n.range.start.line.saturating_sub(1),
                        col: n.range.start.column,
                        offset: n.range.start.offset,
                        ty: Type::named(n.id.clone()),
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
                    let symbol_id = scope.resolve(&t.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: t.id.clone(),
                        kind: ClassMemberKind::Property,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: t.range.start.line.saturating_sub(1),
                        col: t.range.start.column,
                        offset: t.range.start.offset,
                        ty: Type::named_with_origin(
                            t.id.clone(),
                            Some(Rc::from(self.source_file.as_ref())),
                        ),
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
                    let variants = self
                        .type_members
                        .enums
                        .get(&e.id)
                        .cloned()
                        .unwrap_or_default();
                    let symbol_id = scope.resolve(&e.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: e.id.clone(),
                        kind: ClassMemberKind::Enum,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: e.range.start.line.saturating_sub(1),
                        col: e.range.start.column,
                        offset: e.range.start.offset,
                        ty: Type::named(e.id.clone()),
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
                    let symbol_id = scope.resolve(&s.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: s.id.clone(),
                        kind: ClassMemberKind::Struct,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: s.range.start.line.saturating_sub(1),
                        col: s.range.start.column,
                        offset: s.range.start.offset,
                        ty: Type::named(s.id.clone()),
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
                    let interface_members = self
                        .type_members
                        .interfaces
                        .get(&i.id)
                        .cloned()
                        .unwrap_or_default();
                    let symbol_id = scope.resolve(&i.id, &self.scopes);
                    members.push(ClassMemberInfo {
                        name: i.id.clone(),
                        kind: ClassMemberKind::Interface,
                        is_async: false,
                        is_generator: false,
                        is_static: false,
                        is_optional: false,
                        line: i.range.start.line.saturating_sub(1),
                        col: i.range.start.column,
                        offset: i.range.start.offset,
                        ty: Type::named(i.id.clone()),
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
        let mut sym = Symbol::new(SymbolKind::Struct, s.id.clone(), s.range.start.line).with_type(
            Type::named_with_origin(s.id.clone(), Some(Rc::from(self.source_file.as_ref()))),
        );
        sym.doc = s.doc.as_ref().map(|s| Rc::from(s.as_str()));
        self.define(s.id.to_string(), sym);

        let mut members = Vec::new();
        for field in &s.fields {
            let ty = resolve_type_node(&field.type_ann, Some(self));

            let mut field_sym = Symbol::new(
                SymbolKind::Property,
                field.name.clone(),
                field.range.start.line,
            )
            .with_type(ty.clone());
            field_sym.col = field.range.start.column;
            field_sym.offset = field.range.start.offset;
            field_sym.has_explicit_type = true;
            let symbol_id = self.arena.push(field_sym);

            members.push(ClassMemberInfo {
                name: field.name.clone(),
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
            name: s.id.clone(),
            kind: ClassMemberKind::Struct,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: s.range.start.line.saturating_sub(1),
            col: s.range.start.column,
            offset: s.range.start.offset,
            ty: Type::named_with_origin(s.id.clone(), Some(Rc::from(self.source_file.as_ref()))),
            members,
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
            ..Default::default()
        };
        self.type_members.classes.insert(s.id.clone(), struct_info);
    }
}
