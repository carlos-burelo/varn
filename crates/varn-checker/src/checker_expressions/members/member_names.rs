use std::sync::Arc;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(crate) fn collect_member_names(&self, ty: &Type, bind: &BindResult) -> Vec<Arc<str>> {
        let ty_kind = self.ty_table.get(ty.0);
        match ty_kind {
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .filter_map(|m| match m {
                    ObjectTypeMember::Property { name, .. }
                    | ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            TypeKind::Named(cn, _) | TypeKind::Generic(cn, _, _) => {
                let cn_str = super::member_atom::resolve_atom_text(bind, cn);
                bind.get_class_entry(&cn_str)
                    .map(|entry| entry.members.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default()
            }
            TypeKind::Union(list) => {
                let mut names: Vec<Arc<str>> = Vec::new();
                for id in self.ty_table.get_list(list).to_vec() {
                    let m = Type(id, false);
                    if !m.is_nullable(&self.ty_table) {
                        names.extend(self.collect_member_names(&m, bind));
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
