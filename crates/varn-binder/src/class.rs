use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::ClassDecl;
use varn_core::{Atom, TypeKind};
use varn_sem::symbol::{Symbol, SymbolKind};
use varn_sem::types::Type;
use varn_sem::types::{ClassMemberInfo, ClassMemberKind};

impl<'r> super::Binder<'r> {
    pub(super) fn bind_class(&mut self, c: &ClassDecl) {
        let name_atom: Atom = c.id.unwrap_or_else(|| self.intern_local("<anon>"));
        let name: Arc<str> = self.declared_type_name(name_atom, c.ast_id);
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
        sym.type_param_defaults = c
            .type_params
            .iter()
            .map(|t| t.default.as_ref().map(|given| self.resolve_type(given)))
            .collect();
        sym.decorated = !c.decorators.is_empty();
        let sym_id = self.define(name_atom, sym);
        crate::decorator_attrs::record_decorators(self, sym_id, &c.decorators);
        if c.id.is_some() {
            self.note_type_decl(&name, self.current, c.range);
        }

        let child = self
            .scopes
            .child(varn_sem::scope::ScopeKind::Class, self.current);
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
            let super_ty = self.heritage_type(*e);
            let args: Vec<Type> = c
                .super_type_args
                .iter()
                .map(|node| self.resolve_type(node))
                .collect();
            match self.ty_table.get(super_ty.0) {
                TypeKind::Named(n, o) => Some((n, o, args)),
                TypeKind::Generic(n, _, o) => Some((n, o, args)),
                TypeKind::Primitive(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Union(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::TemplateLiteral(_)
                | TypeKind::Fn(_)
                | TypeKind::Object(_)
                | TypeKind::Record(_)
                | TypeKind::Typeof(_)
                | TypeKind::KeyOf(_)
                | TypeKind::IndexedAccess { .. }
                | TypeKind::Mapped { .. }
                | TypeKind::Conditional { .. }
                | TypeKind::Infer(_)
                | TypeKind::EnumVariant { .. }
                | TypeKind::TypePredicate { .. } => None,
            }
        });
        let extends =
            extends.map(|(n, o, args)| (self.name_text(n), o.map(|o| self.name_text(o)), args));

        let mut final_members = members.clone();
        if let Some((parent_name, parent_origin, parent_args)) = extends {
            let parent_is_local = parent_origin
                .as_deref()
                .is_none_or(|o| o == self.source_file.as_ref());
            let owned_members: Vec<ClassMemberInfo> = parent_is_local
                .then(|| self.local_class_members(parent_name.as_ref()))
                .flatten()
                .cloned()
                .unwrap_or_default();
            let mapping = self.parent_param_mapping(&parent_name, &parent_args);
            self.class_parents.insert(
                name.clone(),
                varn_sem::bind::ClassParent {
                    name: Arc::from(parent_name.as_ref()),
                    origin: parent_origin
                        .as_deref()
                        .filter(|_| !parent_is_local)
                        .map(Arc::from),
                    args: parent_args,
                },
            );
            for pm in &owned_members {
                if members.iter().any(|m| m.name == pm.name) {
                    continue;
                }
                if mapping.is_empty() {
                    final_members.push(pm.clone());
                    continue;
                }
                let mut substituted = pm.clone();
                let table = std::sync::Arc::make_mut(&mut self.ty_table);
                substituted.ty = pm.ty.map_generics(&mapping, table);
                final_members.push(substituted);
            }
        }

        self.record_interfaces(&name, &c.implements);

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

        self.declare_kind(name.clone(), varn_sem::bind::DeclKind::Class);
        self.type_members.classes.insert(name, class_info);
        self.current = saved;
    }

    pub(crate) fn local_class_members(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.classes.get(name).map(|e| &e.members)
    }

    fn parent_param_mapping(
        &self,
        parent_name: &str,
        parent_args: &[Type],
    ) -> FxHashMap<Atom, Type> {
        if parent_args.is_empty() {
            return FxHashMap::default();
        }
        let Some(atom) = self.interner.get(parent_name) else {
            return FxHashMap::default();
        };
        let mut scope = self.current;
        while let Some(parent) = self.scopes.get(scope).parent {
            scope = parent;
        }
        let Some(sid) = self.scopes.get(scope).resolve(atom, &self.scopes) else {
            return FxHashMap::default();
        };
        self.arena
            .get(sid)
            .type_params
            .iter()
            .map(|p| Atom::of(&self.interner.resolve(*p)))
            .zip(parent_args.iter().cloned())
            .collect()
    }

    pub(crate) fn local_interface_members(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.interfaces.get(name)
    }
}
