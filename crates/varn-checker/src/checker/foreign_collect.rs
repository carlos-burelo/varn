use super::foreign_classes::OwnerRef;
use super::Checker;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::output::{ForeignClass, InheritedField};
use varn_sem::types::{ClassMemberKind, Type};

impl Checker<'_> {
    pub(super) fn collect_foreign_classes<'t>(
        &mut self,
        bind: &BindResult,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<ForeignClass> {
        let mut found: BTreeMap<(Arc<str>, Arc<str>), ForeignClass> = BTreeMap::new();
        let mut seeds: Vec<Type> = types.copied().collect();
        while let Some(ty) = seeds.pop() {
            for key in self.foreign_refs_in(bind, ty) {
                if found.contains_key(&key) {
                    continue;
                }
                let Some((entry, field_tys)) = self.build_foreign_entry(bind, &key.0, &key.1)
                else {
                    continue;
                };
                seeds.extend(field_tys);
                found.insert(key, entry);
            }
        }
        found.into_values().collect()
    }

    fn foreign_refs_in(&mut self, bind: &BindResult, ty: Type) -> Vec<(Arc<str>, Arc<str>)> {
        let mut out = Vec::new();
        self.collect_refs_in(bind, ty, &mut out);
        out
    }

    fn collect_refs_in(
        &mut self,
        bind: &BindResult,
        ty: Type,
        out: &mut Vec<(Arc<str>, Arc<str>)>,
    ) {
        match self.ty_table.get(ty.0) {
            TypeKind::Union(list) => {
                for id in self.ty_table.get_list(list).to_vec() {
                    self.collect_refs_in(bind, Type::resolved(id), out);
                }
            }
            TypeKind::Generic(_, args, _) => {
                for id in self.ty_table.get_list(args).to_vec() {
                    self.collect_refs_in(bind, Type::resolved(id), out);
                }
                self.collect_named_ref(bind, ty, out);
            }
            TypeKind::Array(el) => {
                self.collect_refs_in(bind, Type::resolved(el), out);
            }
            TypeKind::Tuple(els) => {
                for id in self.ty_table.get_list(els).to_vec() {
                    self.collect_refs_in(bind, Type::resolved(id), out);
                }
            }
            TypeKind::Named(..) => {
                self.collect_named_ref(bind, ty, out);
            }
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Intersection(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => {}
        }
    }

    fn collect_named_ref(
        &mut self,
        bind: &BindResult,
        ty: Type,
        out: &mut Vec<(Arc<str>, Arc<str>)>,
    ) {
        let (TypeKind::Named(name, Some(origin)) | TypeKind::Generic(name, _, Some(origin))) =
            self.ty_table.get(ty.0)
        else {
            return;
        };
        let origin = self.resolve_bind_atom(bind, origin);
        if origin.as_ref() == bind.source_file.as_ref() {
            return;
        }
        let name = self.resolve_bind_atom(bind, name);
        if varn_core::is_lang_type_name(name.as_ref()) {
            return;
        }
        let key = (name, origin);
        if !out.contains(&key) {
            out.push(key);
        }
    }

    fn build_foreign_entry(
        &mut self,
        bind: &BindResult,
        name: &Arc<str>,
        origin: &Arc<str>,
    ) -> Option<(ForeignClass, Vec<Type>)> {
        let owner = self
            .resolver
            .module_bind(origin)
            .or_else(|| self.resolver.stdlib_bind(origin));
        let Some(owner) = owner else {
            return None;
        };
        use varn_sem::bind::DeclKind;
        if !matches!(owner.classify_decl(name.as_ref()), Some(DeclKind::Class)) {
            return None;
        }
        let chain = self.foreign_chain(name.as_ref(), OwnerRef::Foreign(owner.clone()));
        let mut fields = Vec::new();
        let mut methods = Vec::new();

        for level in chain.iter().rev() {
            let Some(entry) = level.owner.get().get_class_entry(&level.name) else {
                continue;
            };
            for m in &entry.members {
                if m.is_static {
                    continue;
                }
                match m.kind {
                    ClassMemberKind::Property | ClassMemberKind::Variable => {
                        if fields.iter().any(|f: &InheritedField| f.name == m.name) {
                            continue;
                        }
                        fields.push(InheritedField {
                            name: m.name.clone(),
                            ty: self.reintern_foreign_ty(level.owner.get(), m.ty),
                            optional: m.is_optional,
                        });
                    }
                    ClassMemberKind::Method | ClassMemberKind::Function => {
                        if !methods.contains(&m.name) {
                            methods.push(m.name.clone());
                        }
                    }
                    ClassMemberKind::Constructor => {}
                    ClassMemberKind::Getter
                    | ClassMemberKind::Setter
                    | ClassMemberKind::Class
                    | ClassMemberKind::Interface
                    | ClassMemberKind::Namespace
                    | ClassMemberKind::Enum
                    | ClassMemberKind::Struct => {}
                }
            }
        }
        let constructor =
            Some(self.effective_ctor_ty(name.as_ref(), OwnerRef::Foreign(owner.clone())));
        methods.sort();
        let decorated = owner
            .interner
            .get(name.as_ref())
            .and_then(|atom| {
                owner
                    .scopes
                    .get(owner.global_scope)
                    .resolve(atom, &owner.scopes)
            })
            .is_none_or(|sid| owner.arena.get(sid).decorated);
        let import_binding = bind
            .global_symbols()
            .find(|s| {
                !decorated
                    && s.origin_module
                        .is_some_and(|m| self.resolve_bind_atom(bind, m) == *origin)
                    && s.original_name
                        .is_some_and(|n| self.resolve_bind_atom(bind, n) == *name)
            })
            .map(|s| self.resolve_bind_atom(bind, s.name));
        let field_tys = fields.iter().map(|f| f.ty).collect();
        Some((
            ForeignClass {
                name: name.clone(),
                origin: origin.clone(),
                fields,
                methods,
                constructor,
                import_binding,
            },
            field_tys,
        ))
    }
}
