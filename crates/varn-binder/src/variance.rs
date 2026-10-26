use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_sem::compat::variance::{measure_params, Variance};
use varn_sem::symbol::SymbolKind;

use super::Binder;

pub(super) fn compute_type_param_variance(b: &Binder) -> FxHashMap<Arc<str>, Vec<Variance>> {
    let mut generics: Vec<(Arc<str>, Vec<Arc<str>>)> = b
        .arena
        .all()
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
            ) && !s.type_params.is_empty()
        })
        .map(|s| {
            let name = Arc::from(b.interner.resolve(s.name));
            let params = s
                .type_params
                .iter()
                .map(|a| Arc::from(b.interner.resolve(*a)))
                .collect();
            (name, params)
        })
        .collect();
    generics.sort_by(|a, b| a.0.cmp(&b.0));
    let empty: Vec<varn_sem::types::ClassMemberInfo> = Vec::new();
    let no_bases: Vec<varn_sem::types::Type> = Vec::new();
    let mut out = FxHashMap::default();
    for (name, params) in &generics {
        let mut payloads = Vec::new();
        if let Some(variants) = b.sum_type_variants.get(name.as_ref()) {
            for variant in variants {
                if let Some(fields) = b.sum_variant_fields.get(variant.as_ref()) {
                    for (field, ty) in fields {
                        payloads.push(varn_sem::types::ClassMemberInfo {
                            name: field.clone(),
                            ty: *ty,
                            is_readonly: true,
                            ..Default::default()
                        });
                    }
                }
            }
            if let Some(entry) = b.type_members.classes.get(name.as_ref()) {
                payloads.extend(
                    entry
                        .members
                        .iter()
                        .filter(|m| {
                            !m.is_static
                                && matches!(
                                    m.kind,
                                    varn_sem::types::ClassMemberKind::Property
                                        | varn_sem::types::ClassMemberKind::Variable
                                )
                        })
                        .cloned(),
                );
            }
        }
        let members: &[varn_sem::types::ClassMemberInfo] =
            if b.sum_type_variants.contains_key(name.as_ref()) {
                &payloads
            } else if let Some(e) = b.type_members.classes.get(name.as_ref()) {
                &e.members
            } else if let Some(list) = b.type_members.interfaces.get(name.as_ref()) {
                list
            } else {
                &empty
            };
        let bases: &[varn_sem::types::Type] = b
            .class_parents
            .get(name.as_ref())
            .map(|p| p.args.as_slice())
            .unwrap_or(&no_bases);
        out.insert(
            name.clone(),
            measure_params(params, members, bases, &b.ty_table, &b.interner),
        );
    }
    out
}
