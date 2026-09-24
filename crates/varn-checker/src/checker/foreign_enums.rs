//! Enums declared in another module whose values this module handles (the
//! core `Option`/`Result`, an imported sum). The backend needs their tags to
//! build, test and destructure those values directly; it gets them from the
//! declaring module's layout, the same one that module's own backend used.

use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_core::TypeKind;

/// A foreign enum: its name, the module declaring it, and its variants in tag
/// order with their payload arity.
#[derive(Clone, Debug)]
pub struct ForeignEnum {
    pub name: Arc<str>,
    pub origin: Arc<str>,
    pub variants: Vec<(Arc<str>, usize)>,
}

impl Checker<'_> {
    /// Every foreign enum among `types`, in a deterministic order.
    pub(super) fn collect_foreign_enums<'t>(
        &self,
        bind: &BindResult,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<ForeignEnum> {
        let mut found: BTreeMap<(Arc<str>, Arc<str>), ForeignEnum> = BTreeMap::new();
        for ty in types {
            let members = match self.ty_table.get(ty.0) {
                TypeKind::Union(list) => self.ty_table.get_list(list).to_vec(),
                _ => vec![ty.0],
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
                let key = (name.clone(), origin.clone());
                if found.contains_key(&key) {
                    continue;
                }
                let owner = self
                    .resolver
                    .module_bind(&origin)
                    .or_else(|| self.resolver.stdlib_bind(&origin));
                let Some(layout) = owner.and_then(|b| b.enum_layout(&name)) else {
                    continue;
                };
                let variants = layout.into_iter().map(|(v, fields)| (v, fields.len())).collect();
                found.insert(
                    key,
                    ForeignEnum {
                        name,
                        origin,
                        variants,
                    },
                );
            }
        }
        found.into_values().collect()
    }
}
