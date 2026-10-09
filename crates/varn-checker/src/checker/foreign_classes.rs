use super::Checker;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_core::TypeKind;
use varn_sem::bind::{BindResult, ClassParent};
use varn_sem::output::{ForeignClass, InheritedField};
use varn_sem::types::{ClassMemberKind, Type};

impl Checker<'_> {
    pub(super) fn collect_foreign_inherited_fields(
        &mut self,
        bind: &BindResult,
    ) -> BTreeMap<Arc<str>, Vec<InheritedField>> {
        let mut classes: Vec<(Arc<str>, ClassParent)> = bind
            .class_parents
            .iter()
            .filter(|(_, p)| p.origin.is_some() || !bind.type_members.classes.contains_key(&p.name))
            .map(|(c, p)| (c.clone(), p.clone()))
            .collect();
        classes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = BTreeMap::new();
        for (class, parent) in classes {
            let mut fields = Vec::new();
            let mut visited = Vec::new();
            self.ancestor_fields(parent, bind, &mut fields, &mut visited);
            out.insert(class, fields);
        }
        out
    }

    pub(super) fn collect_foreign_classes<'t>(
        &mut self,
        bind: &BindResult,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<ForeignClass> {
        let mut found: BTreeMap<(Arc<str>, Arc<str>), ForeignClass> = BTreeMap::new();
        for ty in types {
            let members = match self.ty_table.get(ty.0) {
                TypeKind::Union(list) => self.ty_table.get_list(list).to_vec(),
                TypeKind::Primitive(_)
                | TypeKind::Builtin(_)
                | TypeKind::Literal(_)
                | TypeKind::This
                | TypeKind::Array(_)
                | TypeKind::Intersection(_)
                | TypeKind::Tuple(_)
                | TypeKind::Named(..)
                | TypeKind::Generic(..)
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
                | TypeKind::TypePredicate { .. } => vec![ty.0],
            };
            for id in members {
                let (TypeKind::Named(name, Some(origin))
                | TypeKind::Generic(name, _, Some(origin))) = self.ty_table.get(id)
                else {
                    continue;
                };
                let origin = self.resolve_bind_atom(bind, origin);
                if origin.as_ref() == bind.source_file.as_ref() {
                    continue;
                }
                let name = self.resolve_bind_atom(bind, name);
                if varn_core::is_lang_type_name(name.as_ref()) {
                    continue;
                }
                let key = (name.clone(), origin.clone());
                if found.contains_key(&key) {
                    continue;
                }
                let owner = self
                    .resolver
                    .module_bind(&origin)
                    .or_else(|| self.resolver.stdlib_bind(&origin));
                let Some(owner) = owner.as_deref() else {
                    continue;
                };
                if owner.sum_type_variants.contains_key(name.as_ref())
                    || owner.type_members.enums.contains_key(name.as_ref())
                {
                    continue;
                }
                if owner.get_class_parent(name.as_ref()).is_some() {
                    continue;
                }
                let Some(entry) = owner.get_class_entry(&name) else {
                    continue;
                };
                if entry.is_builtin_or_intrinsic {
                    continue;
                }
                let mut fields = Vec::new();
                let mut methods = Vec::new();
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
                                ty: self.reintern_foreign_ty(owner, m.ty),
                                optional: m.is_optional,
                            });
                        }
                        ClassMemberKind::Method | ClassMemberKind::Function => {
                            if !methods.contains(&m.name) {
                                methods.push(m.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
                methods.sort();
                found.insert(
                    key,
                    ForeignClass {
                        name,
                        origin,
                        fields,
                        methods,
                    },
                );
            }
        }
        found.into_values().collect()
    }

    fn ancestor_fields(
        &mut self,
        class: ClassParent,
        bind: &BindResult,
        out: &mut Vec<InheritedField>,
        visited: &mut Vec<ClassParent>,
    ) {
        if visited.contains(&class) {
            return;
        }
        visited.push(class.clone());
        let owner = class.origin.as_deref().and_then(|o| {
            self.resolver
                .module_bind(o)
                .or_else(|| self.resolver.stdlib_bind(o))
        });
        let declaring = owner.as_deref().unwrap_or(bind);
        let members = declaring
            .get_class_entry(&class.name)
            .map(|e| e.members.clone())
            .unwrap_or_default();
        let parent = declaring
            .get_class_parent(&class.name)
            .map(|p| ClassParent {
                name: p.name.clone(),
                origin: p.origin.clone().or_else(|| class.origin.clone()),
            });
        if let Some(parent) = parent {
            self.ancestor_fields(parent, bind, out, visited);
        }
        for m in members {
            if m.is_static
                || !matches!(
                    m.kind,
                    ClassMemberKind::Property | ClassMemberKind::Variable
                )
                || out.iter().any(|f| f.name == m.name)
            {
                continue;
            }
            let ty = match &owner {
                Some(owner) => self.reintern_foreign_ty(owner, m.ty),
                None => m.ty,
            };
            out.push(InheritedField {
                name: m.name.clone(),
                ty,
                optional: m.is_optional,
            });
        }
    }
}
