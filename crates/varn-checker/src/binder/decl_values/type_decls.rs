use crate::binder::{ClassMemberInfo, ClassMemberKind, PendingEnrich};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use std::sync::Arc;
use varn_core::ast::{ClassMember, EnumDecl, TypeAliasDecl};

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_type_alias(&mut self, t: &TypeAliasDecl) {
        let has_type_params = !t.type_params.is_empty();

        let ty = if has_type_params {
            crate::types::Type::Dynamic
        } else {
            self.resolve_type(&t.alias)
        };
        let mut sym = Symbol::new(SymbolKind::TypeAlias, t.id, t.range.start.line).with_type(ty);
        sym.offset = t.range.start.offset;
        sym.col = t.range.start.column;
        sym.doc = t.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        sym.type_params = t.type_params.iter().map(|tp| tp.name).collect();
        if has_type_params {
            sym.alias_node = Some(Box::new(t.alias.clone()));
        }
        self.define(t.id, sym);
    }

    pub(crate) fn bind_enum(&mut self, e: &EnumDecl) {
        let line = e.range.start.line;
        let id_rc: Arc<str> = Arc::from(self.interner.resolve(e.id));
        self.note_type_decl(&id_rc, self.current, e.range);
        let mut sym = Symbol::new(SymbolKind::Enum, e.id, line).with_type(Type::named_with_origin(
            id_rc.clone(),
            Some(Arc::from(self.source_file.as_ref())),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        ));
        sym.doc = e.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        sym.type_params = e.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = e
            .type_params
            .iter()
            .map(|t| t.constraint.as_ref().map(|con| self.resolve_type(con)))
            .collect();
        self.define(e.id, sym);

        self.sum_type_variants.insert(
            Arc::from(self.interner.resolve(e.id)),
            e.members
                .iter()
                .map(|m| Arc::from(self.interner.resolve(m.id)))
                .collect(),
        );

        let mut variants_info = Vec::new();

        for member in e.members.iter() {
            let member_id_rc: Arc<str> = Arc::from(self.interner.resolve(member.id));
            let fields: Vec<(Arc<str>, Type)> = member
                .payload_fields
                .iter()
                .map(|f| {
                    let ty = self.resolve_type(&f.ty);
                    (Arc::from(self.interner.resolve(f.name)), ty)
                })
                .collect();

            self.sum_variant_parent
                .insert(member_id_rc.clone(), id_rc.clone());
            self.sum_variant_fields
                .insert(member_id_rc.clone(), fields.clone());

            let variant_sym_id = if member.payload_fields.is_empty() {
                let variant_ty = Type::named_with_origin(
                    id_rc.clone(),
                    Some(Arc::from(self.source_file.as_ref())),
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                let v_sym = Symbol::new(SymbolKind::EnumMember, member.id, member.range.start.line)
                    .with_type(variant_ty);
                self.define(member.id, v_sym)
            } else {
                let params: Vec<crate::types::FunctionParam> = fields
                    .iter()
                    .map(|(fname, fty)| crate::types::FunctionParam {
                        name: Some(fname.clone()),
                        ty: fty.0,
                        optional: false,
                        is_rest: false,
                    })
                    .collect();
                let ret_ty = Type::named_with_origin(
                    id_rc.clone(),
                    Some(Arc::from(self.source_file.as_ref())),
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                let fn_ty = Type::fn_(
                    crate::types::FunctionType {
                        params,
                        return_type: ret_ty.0,
                        is_arrow: false,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                let v_sym = Symbol::new(SymbolKind::EnumMember, member.id, member.range.start.line)
                    .with_type(fn_ty);
                self.define(member.id, v_sym)
            };

            variants_info.push(ClassMemberInfo {
                name: member_id_rc,
                kind: ClassMemberKind::Property,
                is_async: false,
                is_generator: false,
                is_static: true,
                is_optional: false,
                line: member.range.start.line.saturating_sub(1),
                col: member.range.start.column,
                offset: member.range.start.offset,
                ty: Type::named_with_origin(
                    id_rc.clone(),
                    Some(Arc::from(self.source_file.as_ref())),
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                ),
                members: Vec::new(),
                visibility: None,
                is_abstract: false,
                is_readonly: false,
                is_override: false,
                symbol_id: Some(variant_sym_id),
                ..Default::default()
            });
        }

        let child = self
            .scopes
            .child(crate::scope::ScopeKind::Class, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&e.type_params, line);

        let mut methods: rustc_hash::FxHashMap<Arc<str>, Type> = rustc_hash::FxHashMap::default();
        let mut members: Vec<ClassMemberInfo> = Vec::new();

        for member in &e.body {
            self.collect_class_member(member, id_rc.as_ref(), &mut methods, &mut members);
        }

        for member in &e.body {
            match member {
                ClassMember::Constructor {
                    params,
                    body,
                    range,
                    ..
                } => {
                    self.bind_inline_function(&[], params, None, *body, range);
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
                    let body = *body;
                    if return_type.is_none() && !modifiers.is_abstract {
                        self.pending_enrich.push(PendingEnrich::Method {
                            class_name: e.id,
                            key: *key,
                            body,
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
                    ..
                } => {
                    let body = *body;
                    if return_type.is_none() {
                        self.pending_enrich.push(PendingEnrich::Getter {
                            class_name: e.id,
                            key: *key,
                            body,
                        });
                    }
                    // See array_evolve rule 3: getters/setters are closures
                    // but don't create their own Function scope, so the
                    // escape has to be applied explicitly.
                    self.escape_all_open_array_candidates();
                    self.bind_stmt(body);
                }
                ClassMember::Setter {
                    key,
                    param,
                    body: Some(body),
                    range,
                    ..
                } => {
                    let body = *body;
                    self.pending_enrich.push(PendingEnrich::Setter {
                        class_name: e.id,
                        key: *key,
                        body,
                    });
                    self.escape_all_open_array_candidates();
                    self.bind_stmt(body);
                    let ty = param.type_ann.as_ref().map(|ann| self.resolve_type(ann));
                    self.bind_pattern(
                        &param.pattern,
                        SymbolKind::Parameter,
                        range.start.line,
                        None,
                        ty,
                        param.type_ann.is_some(),
                    );
                }
                ClassMember::Property {
                    init: Some(init), ..
                } => {
                    self.bind_expr(*init);
                }
                _ => {}
            }
        }

        let class_info = ClassMemberInfo {
            name: id_rc.clone(),
            kind: ClassMemberKind::Class,
            is_async: false,
            is_generator: false,
            is_static: false,
            is_optional: false,
            line: e.range.start.line.saturating_sub(1),
            col: e.range.start.column,
            offset: e.range.start.offset,
            ty: Type::named_with_origin(
                id_rc.clone(),
                Some(Arc::from(self.source_file.as_ref())),
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            members: members.clone(),
            visibility: None,
            is_abstract: false,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
            ..Default::default()
        };

        self.type_members.classes.insert(id_rc.clone(), class_info);

        variants_info.extend(members);
        if !variants_info.is_empty() {
            self.type_members.enums.insert(id_rc, variants_info);
        }

        self.current = saved;
    }
}
