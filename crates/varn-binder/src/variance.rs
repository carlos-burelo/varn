use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::DeclaredVariance;
use varn_core::source::{SourceLocation, SourceRange};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::compat::variance::{measure_params, Variance};
use varn_sem::symbol::SymbolKind;

use super::Binder;

struct Generic {
    name: Arc<str>,
    params: Vec<Arc<str>>,
    declared: Vec<Option<DeclaredVariance>>,
    at: SourceRange,
}

fn variance_of(declared: Option<DeclaredVariance>) -> Variance {
    match declared {
        Some(DeclaredVariance::Out) => Variance::Covariant,
        Some(DeclaredVariance::In) => Variance::Contravariant,
        None => Variance::Invariant,
    }
}

pub(super) fn compute_type_param_variance(
    b: &Binder,
) -> (FxHashMap<Arc<str>, Vec<Variance>>, Vec<Diagnostic>) {
    let mut generics: Vec<Generic> = b
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
            let at = SourceLocation::new(s.line, s.col, s.offset);
            Generic {
                name: Arc::from(b.interner.resolve(s.name)),
                params: s
                    .type_params
                    .iter()
                    .map(|a| Arc::from(b.interner.resolve(*a)))
                    .collect(),
                declared: s.type_param_variances.clone(),
                at: SourceRange { start: at, end: at },
            }
        })
        .collect();
    generics.sort_by(|a, b| a.name.cmp(&b.name));

    let declared_variances: FxHashMap<Arc<str>, Vec<Variance>> = generics
        .iter()
        .map(|g| {
            let list = (0..g.params.len())
                .map(|i| variance_of(g.declared.get(i).copied().flatten()))
                .collect();
            (g.name.clone(), list)
        })
        .collect();

    let empty: Vec<varn_sem::types::ClassMemberInfo> = Vec::new();
    let mut diagnostics = Vec::new();
    for g in &generics {
        let payloads = sum_type_payloads(b, &g.name);
        let members: &[varn_sem::types::ClassMemberInfo] =
            if b.sum_type_variants.contains_key(g.name.as_ref()) {
                &payloads
            } else if let Some(e) = b.type_members.classes.get(g.name.as_ref()) {
                &e.members
            } else if let Some(list) = b.type_members.interfaces.get(g.name.as_ref()) {
                list
            } else {
                &empty
            };
        let bases: Vec<(varn_sem::types::Type, Variance)> = b
            .class_parents
            .get(g.name.as_ref())
            .map(|parent| {
                let positions = declared_variances.get(parent.name.as_ref());
                parent
                    .args
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| {
                        let variance = positions
                            .and_then(|list| list.get(i).copied())
                            .unwrap_or(Variance::Invariant);
                        (*ty, variance)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let uses = measure_params(
            &g.params,
            members,
            &bases,
            &b.ty_table,
            &b.interner,
            &declared_variances,
        );
        for (i, declared) in g.declared.iter().enumerate() {
            let Some(use_) = uses.get(i) else { continue };
            let broken = match declared {
                Some(DeclaredVariance::Out) => use_.ins.then_some(("out", "input")),
                Some(DeclaredVariance::In) => use_.outs.then_some(("in", "output")),
                None => None,
            };
            if let Some((keyword, position)) = broken {
                diagnostics.push(
                    Diagnostic::error(
                        ErrorCode::VarianceViolation,
                        format!(
                            "type parameter '{}' of '{}' is declared `{keyword}` but is used in an {position} position",
                            g.params[i], g.name
                        ),
                    )
                    .with_range(g.at)
                    .with_file(b.source_file.clone()),
                );
            }
        }
    }
    (declared_variances, diagnostics)
}

fn sum_type_payloads(b: &Binder, name: &Arc<str>) -> Vec<varn_sem::types::ClassMemberInfo> {
    let mut payloads = Vec::new();
    let Some(variants) = b.sum_type_variants.get(name.as_ref()) else {
        return payloads;
    };
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
    payloads
}
