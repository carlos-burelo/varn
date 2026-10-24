use super::Checker;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::output::{ForeignEnum, ForeignEnumMember};
use varn_sem::types::Type;

impl Checker<'_> {
    pub(super) fn collect_foreign_enums<'t>(
        &mut self,
        bind: &BindResult,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<ForeignEnum> {
        let mut found: BTreeMap<(Arc<str>, Arc<str>), ForeignEnum> = BTreeMap::new();
        let keys: Vec<(Arc<str>, Arc<str>)> = types
            .flat_map(|ty| self.foreign_refs_in(bind, *ty))
            .collect();
        for key in keys {
            let (name, origin) = key.clone();
            if found.contains_key(&key) {
                continue;
            }
            let owner = self
                .resolver
                .module_bind(&origin)
                .or_else(|| self.resolver.stdlib_bind(&origin));
            let Some((owner, layout)) = owner.as_deref().and_then(|b| {
                use varn_sem::bind::DeclKind;
                if !matches!(
                    b.classify_decl(name.as_ref()),
                    Some(DeclKind::Enum) | Some(DeclKind::SumType)
                ) {
                    return None;
                }
                Some((b, b.enum_layout(&name)?))
            }) else {
                continue;
            };
            let variants = layout
                .into_iter()
                .map(|(variant, fields)| {
                    let payload = fields
                        .iter()
                        .map(|(_, ty)| self.reintern_foreign_ty(owner, *ty))
                        .collect();
                    (variant, payload)
                })
                .collect();
            let type_params = Self::declared_type_params(owner, &name);
            let members = owner
                .get_enum_members_local(name.as_ref())
                .map(|members| {
                    members
                        .iter()
                        .filter(|m| !m.is_static)
                        .map(|m| ForeignEnumMember {
                            kind: m.kind,
                            name: m.name.clone(),
                            ty: self.reintern_foreign_ty(owner, m.ty),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let binding = bind
                .global_symbols()
                .find(|s| {
                    s.origin_module
                        .is_some_and(|m| self.resolve_bind_atom(bind, m) == origin)
                        && s.original_name
                            .is_some_and(|n| self.resolve_bind_atom(bind, n) == name)
                })
                .map(|s| self.resolve_bind_atom(bind, s.name));
            found.insert(
                key,
                ForeignEnum {
                    name,
                    origin,
                    binding,
                    type_params,
                    variants,
                    members,
                },
            );
        }
        found.into_values().collect()
    }

    fn declared_type_params(owner: &BindResult, name: &str) -> Vec<Arc<str>> {
        owner
            .interner
            .get(name)
            .and_then(|atom| {
                owner
                    .scopes
                    .get(owner.global_scope)
                    .resolve(atom, &owner.scopes)
            })
            .map(|id| {
                owner
                    .arena
                    .get(id)
                    .type_params
                    .iter()
                    .map(|p| Arc::from(owner.interner.resolve(*p)))
                    .collect()
            })
            .unwrap_or_default()
    }
}
