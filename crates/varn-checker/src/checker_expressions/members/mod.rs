mod member_exists;
mod member_type;

use std::rc::Rc;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use varn_core::TypeKind;

impl Checker {
    pub(crate) fn collect_member_names(&self, ty: &Type, bind: &BindResult) -> Vec<Rc<str>> {
        match &ty.0 {
            TypeKind::Object(members) => members
                .iter()
                .filter_map(|m| match m {
                    ObjectTypeMember::Property { name, .. }
                    | ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            TypeKind::Named(cn, _) | TypeKind::Generic(cn, _, _) => bind
                .get_class_entry(cn.as_ref())
                .map(|entry| entry.members.iter().map(|m| m.name.clone()).collect())
                .unwrap_or_default(),
            TypeKind::Union(members) => {
                let mut names: Vec<Rc<str>> = Vec::new();
                for m in members {
                    if !m.is_nullable() {
                        names.extend(self.collect_member_names(m, bind));
                    }
                }
                names.sort_unstable();
                names.dedup();
                names
            }
            _ => Vec::new(),
        }
    }
}
