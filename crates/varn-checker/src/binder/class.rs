use crate::binder::{ClassMemberInfo, ClassMemberKind};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::ClassDecl;
use varn_core::{Atom, TypeKind};

impl<'r> super::Binder<'r> {
    pub(super) fn bind_class(&mut self, c: &ClassDecl) {
        // `name_atom` forwards the class identifier's `Atom` for fields that
        // stay `Atom`-keyed (`PendingEnrich`); `name` is the `Arc<str>` text
        // resolved from it, needed everywhere this still feeds an unmigrated
        // `Arc<str>`-typed API (`Symbol::new`, `ClassMemberInfo`, `Type`).
        let name_atom: Atom = c.id.unwrap_or_else(|| self.intern_local("<anon>"));
        let name: Arc<str> = Arc::from(self.interner.resolve(name_atom));
        let line = c.range.start.line;
        let cls_type = Type::named_with_origin(
            name.clone(),
            Some(Arc::from(self.source_file.as_ref())),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let mut sym = Symbol::new(SymbolKind::Class, name_atom, line).with_type(cls_type);
        sym.col = c.range.start.column;
        sym.offset = c.range.start.offset;
        sym.doc = c.doc.as_ref().map(|s| self.intern_local(s.as_str()));
        sym.type_params = c.type_params.iter().map(|t| t.name).collect();
        sym.type_param_constraints = c
            .type_params
            .iter()
            .map(|t| t.constraint.as_ref().map(|con| self.resolve_type(con)))
            .collect();
        self.define(name_atom, sym);
        if c.id.is_some() {
            self.note_type_decl(&name, self.current, c.range);
        }

        let child = self
            .scopes
            .child(crate::scope::ScopeKind::Class, self.current);
        let saved = self.current;
        self.current = child;

        self.bind_type_params(&c.type_params, line);

        let mut methods: FxHashMap<Arc<str>, Type> = FxHashMap::default();
        let mut members: Vec<ClassMemberInfo> = Vec::new();

        self.bind_primary_constructor(c, &mut members);

        for member in &c.body {
            self.collect_class_member(member, name.as_ref(), &mut methods, &mut members);
        }

        self.bind_class_bodies(c, name_atom);
        self.mark_optional_fields(c, &mut members);

        let extends = c.super_class.as_ref().and_then(|e| {
            let super_ty = self.infer_expr_type_self(*e);
            match self.ty_table.get(super_ty.0) {
                TypeKind::Named(n, o) => Some((n, o)),
                TypeKind::Generic(n, _, o) => Some((n, o)),
                _ => None,
            }
        });
        let extends = extends.map(|(n, o)| (self.name_text(n), o.map(|o| self.name_text(o))));

        let mut final_members = members.clone();
        if let Some((parent_name, parent_origin)) = extends {
            self.class_parents
                .insert(name.clone(), Arc::from(parent_name.as_ref()));
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
            is_abstract: c.modifiers.is_abstract,
            is_readonly: false,
            is_override: false,
            symbol_id: None,
            ..Default::default()
        };

        self.type_members.classes.insert(name, class_info);
        self.current = saved;
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
            let exports = self.resolver.module_exports(origin, &mut visiting);
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
            let exports = self.resolver.module_exports(origin, &mut visiting);
            if let Some(sym) = exports.get(name) {
                if sym.kind == SymbolKind::Interface {
                    return self.type_members.interfaces.get(name);
                }
            }
        }
        None
    }
}
