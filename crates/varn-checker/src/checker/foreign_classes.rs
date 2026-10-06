use super::Checker;
use crate::binder::{BindResult, ClassParent};
use crate::types::{ClassMemberKind, Type};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct InheritedField {
    pub name: Arc<str>,
    pub ty: Type,
    pub optional: bool,
}

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
