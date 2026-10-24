use super::Checker;
use std::collections::BTreeMap;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::output::ForeignEnum;
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
            let Some(layout) = owner.as_deref().and_then(|b| {
                use varn_sem::bind::DeclKind;
                if !matches!(
                    b.classify_decl(name.as_ref()),
                    Some(DeclKind::Enum) | Some(DeclKind::SumType)
                ) {
                    return None;
                }
                b.enum_layout(&name)
            }) else {
                continue;
            };
            let variants = layout
                .into_iter()
                .map(|(v, fields)| (v, fields.len()))
                .collect();
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
                    variants,
                },
            );
        }
        found.into_values().collect()
    }
}
