use super::super::type_inference::widen_literal;
use crate::binder::{ClassMemberInfo, ClassMemberKind, PendingEnrich};
use crate::scope::ScopeKind;
use crate::symbol::{Symbol, SymbolKind};
use crate::types::{FunctionType, Type};
use std::rc::Rc;
use varn_core::ast::{
    ClassMember, EnumDecl, ExprKind, FunctionDecl, Pattern, TypeAliasDecl, VarKind, VariableDecl,
};

impl<'r> super::super::Binder<'r> {
    pub(crate) fn bind_variable(&mut self, v: &VariableDecl) {
        let sym_kind = match v.kind {
            VarKind::Const => SymbolKind::Const,
            VarKind::Let => SymbolKind::Let,
        };
        for d in &v.declarators {
            let line = d.range.start.line;
            let has_explicit_ann = d.type_ann.is_some()
                || matches!(
                    &d.id,
                    Pattern::Identifier {
                        type_ann: Some(_),
                        ..
                    }
                );
            let ty = d
                .type_ann
                .as_ref()
                .or(match &d.id {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|ann| self.resolve_type(ann))
                .or_else(|| {
                    d.init
                        .map(|e| self.infer_expr_type_self(e))
                        .map(|t| {
                            if sym_kind == SymbolKind::Let && !has_explicit_ann {
                                widen_literal(t)
                            } else {
                                t
                            }
                        })
                });

            let needs_enrich =
                !has_explicit_ann && (ty.is_none() || ty.as_ref().is_some_and(|t| t.is_dynamic()));
            self.bind_pattern(&d.id, sym_kind, line, v.doc.clone(), ty);

            if let Pattern::Identifier { name, .. } = &d.id {
                if let Some(init) = d.init {
                    if let ExprKind::Object { properties } = &self.ast_arena.expr(init).kind {
                        let fields = self.collect_object_members(properties);
                        if !fields.is_empty() {
                            self.type_members.objects.insert(*name, fields);
                        }
                    }
                }
            }

            if let Some(init_expr) = d.init {
                if needs_enrich
                    && matches!(
                        &self.ast_arena.expr(init_expr).kind,
                        ExprKind::Call { .. }
                            | ExprKind::Await { .. }
                            | ExprKind::Member { .. }
                            | ExprKind::New { .. }
                            | ExprKind::Match { .. }
                            | ExprKind::Pipeline { .. }
                    )
                {
                    let scope = self.scopes.get(self.current);
                    let name_atom = if let Pattern::Identifier { name, .. } = &d.id {
                        Some(*name)
                    } else {
                        None
                    };
                    if let Some(sym_id) = name_atom.and_then(|n| scope.lookup(n)) {
                        self.pending_enrich.push(PendingEnrich::Var {
                            sym_id,
                            init: init_expr,
                        });
                    }
                } else if !has_explicit_ann
                    && matches!(&self.ast_arena.expr(init_expr).kind, ExprKind::Array { elements } if elements.is_empty())
                {
                    // Task A0.3': `let`/`const x = []` (empty literal, no
                    // annotation) — register as a candidate for evolving
                    // element-type inference (see `binder::array_evolve`).
                    // Module top-level qualifies too: what a single-file
                    // scan cannot account for is a binding that LEAVES the
                    // file, and that is exactly what `bind_export` escapes.
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let scope = self.scopes.get(self.current);
                        if let Some(sym_id) = scope.lookup(*name) {
                            self.register_array_candidate(sym_id, *name);
                        }
                    }
                }
                self.bind_expr(init_expr);
            }
        }
    }

    pub(crate) fn bind_function(&mut self, f: &FunctionDecl) {
        let line = f.range.start.line;
        let params: Vec<crate::types::FunctionParam> = f
            .params
            .iter()
            .map(|p| {
                let name = Some(Rc::from(crate::binder::pattern_lead_name(
                    &p.pattern,
                    &self.interner,
                )));
                let mut ty = p
                    .type_ann
                    .as_ref()
                    .or(match &p.pattern {
                        Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    })
                    .map(|ann| self.resolve_type(ann))
                    .or_else(|| {
                        p.default
                            .map(|e| widen_literal(self.infer_expr_type_self(e)))
                    })
                    .unwrap_or(Type::Dynamic);

                if p.is_rest {
                    let is_array = matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_));
                    if !is_array {
                        ty = Type::array(ty, &mut self.ty_table);
                    }
                }

                crate::types::FunctionParam {
                    name,
                    ty: ty.0,
                    optional: p.is_optional || p.default.is_some(),
                    is_rest: p.is_rest,
                }
            })
            .collect();

        let declared_ret = f
            .return_type
            .as_ref()
            .map(|ann| self.resolve_type(ann));

        let ret = if f.modifiers.is_generator {
            crate::types::generator_of(
                declared_ret.unwrap_or(Type::Dynamic),
                f.modifiers.is_async,
                &mut self.ty_table,
                Some(self.resolver),
            )
        } else {
            crate::types::async_fn_return(
                declared_ret.unwrap_or(Type::Void),
                f.modifiers.is_async,
                &mut self.ty_table,
                &self.interner,
                Some(self.resolver),
            )
        };
        let fn_type = Type::fn_(
            FunctionType {
                params: params.clone(),
                return_type: ret.0,
                is_arrow: false,
                type_params: f
                    .type_params
                    .iter()
                    .map(|t| Rc::from(self.interner.resolve(t.name)))
                    .collect(),
            },
            &mut self.ty_table,
        );

        let mut sym = Symbol::new(SymbolKind::Function, f.id, line).with_type(fn_type);
        sym.col = f.range.start.column + (f.id_offset - f.range.start.offset);
        sym.offset = f.id_offset;
        sym.has_explicit_type = f.return_type.is_some();
        sym.is_async = f.modifiers.is_async;
        sym.is_generator = f.modifiers.is_generator;
        sym.doc = f.doc.as_ref().map(|s| self.interner.intern(s.as_str()));
        sym.type_params = f.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = f
            .type_params
            .iter()
            .map(|t| {
                t.constraint
                    .as_ref()
                    .map(|c| self.resolve_type(c))
            })
            .collect();

        let sym_id = self.define(f.id, sym);

        if f.return_type.is_none() && !f.modifiers.is_declare && !f.modifiers.is_generator {
            self.pending_enrich.push(PendingEnrich::Fn {
                sym_id,
                body: f.body,
                is_async: f.modifiers.is_async,
            });
        }

        let child = self.scopes.child(ScopeKind::Function, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&f.type_params, line);

        for p in f.params.iter() {
            let mut ty = p
                .type_ann
                .as_ref()
                .or(match &p.pattern {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|ann| self.resolve_type(ann))
                .or_else(|| {
                    p.default
                        .map(|e| widen_literal(self.infer_expr_type_self(e)))
                })
                .unwrap_or(Type::Dynamic);

            if p.is_rest {
                let is_array = matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_));
                if !is_array {
                    ty = Type::array(ty, &mut self.ty_table);
                }
            }

            self.bind_pattern(
                &p.pattern,
                SymbolKind::Parameter,
                line,
                f.doc.clone(),
                Some(ty),
            );
        }

        if !f.modifiers.is_declare {
            self.bind_stmt(f.body);
        }
        self.current = saved;

        let _ = sym_id;
    }

    pub(crate) fn bind_type_alias(&mut self, t: &TypeAliasDecl) {
        let has_type_params = !t.type_params.is_empty();

        let ty = if has_type_params {
            crate::types::Type::Dynamic
        } else {
            self.resolve_type(&t.alias)
        };
        let mut sym =
            Symbol::new(SymbolKind::TypeAlias, t.id, t.range.start.line).with_type(ty);
        sym.offset = t.range.start.offset;
        sym.col = t.range.start.column;
        sym.doc = t.doc.as_ref().map(|s| self.interner.intern(s.as_str()));
        sym.type_params = t.type_params.iter().map(|tp| tp.name).collect();
        if has_type_params {
            sym.alias_node = Some(Box::new(t.alias.clone()));
        }
        self.define(t.id, sym);
    }

    pub(crate) fn bind_enum(&mut self, e: &EnumDecl) {
        let line = e.range.start.line;
        let id_rc: Rc<str> = Rc::from(self.interner.resolve(e.id));
        let mut sym = Symbol::new(SymbolKind::Enum, e.id, line).with_type(Type::named_with_origin(
            id_rc.clone(),
            Some(Rc::from(self.source_file.as_ref())),
            self.resolver,
            &mut self.ty_table,
        ));
        sym.doc = e.doc.as_ref().map(|s| self.interner.intern(s.as_str()));
        sym.type_params = e.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = e
            .type_params
            .iter()
            .map(|t| {
                t.constraint
                    .as_ref()
                    .map(|con| self.resolve_type(con))
            })
            .collect();
        self.define(e.id, sym);

        self.sum_type_variants.insert(
            Rc::from(self.interner.resolve(e.id)),
            e.members
                .iter()
                .map(|m| Rc::from(self.interner.resolve(m.id)))
                .collect(),
        );

        let mut variants_info = Vec::new();

        for member in e.members.iter() {
            let member_id_rc: Rc<str> = Rc::from(self.interner.resolve(member.id));
            let fields: Vec<(Rc<str>, Type)> = member
                .payload_fields
                .iter()
                .map(|f| {
                    let ty = self.resolve_type(&f.ty);
                    (Rc::from(self.interner.resolve(f.name)), ty)
                })
                .collect();

            self.sum_variant_parent
                .insert(member_id_rc.clone(), id_rc.clone());
            self.sum_variant_fields
                .insert(member_id_rc.clone(), fields.clone());

            let variant_sym_id = if member.payload_fields.is_empty() {
                let variant_ty = Type::named(id_rc.clone(), self.resolver, &mut self.ty_table);
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
                    Some(Rc::from(self.source_file.as_ref())),
                    self.resolver,
                    &mut self.ty_table,
                );
                let fn_ty = Type::fn_(
                    crate::types::FunctionType {
                        params,
                        return_type: ret_ty.0,
                        is_arrow: false,
                        type_params: vec![],
                    },
                    &mut self.ty_table,
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
                ty: Type::named(id_rc.clone(), self.resolver, &mut self.ty_table),
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

        let mut methods: rustc_hash::FxHashMap<Rc<str>, Type> = rustc_hash::FxHashMap::default();
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
                Some(Rc::from(self.source_file.as_ref())),
                self.resolver,
                &mut self.ty_table,
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
