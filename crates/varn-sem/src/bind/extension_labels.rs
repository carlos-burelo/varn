use crate::types::{CheckerTyTable, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstId, ExtensionDecl, ExtensionMember};
use varn_core::extension_names::ExtensionKind;
use varn_core::AtomInterner;

pub struct ExtensionName {
    pub kind: ExtensionKind,
    pub member: Arc<str>,
    pub mangled: Arc<str>,
}

pub fn extension_label(
    target: AstId,
    annotation_types: &FxHashMap<AstId, Type>,
    table: &CheckerTyTable,
    interner: &AtomInterner,
) -> Option<Arc<str>> {
    annotation_types
        .get(&target)?
        .extension_key(table, &|atom| {
            interner
                .try_resolve(atom)
                .or_else(|| table.name(atom))
                .map(Arc::from)
        })
}

pub fn extension_names(
    ext: &ExtensionDecl,
    label: &str,
    interner: &AtomInterner,
) -> Vec<ExtensionName> {
    ext.members
        .iter()
        .map(|member| {
            let (kind, member) = match member {
                ExtensionMember::Method(f) => (ExtensionKind::Method, interner.resolve(f.id)),
                ExtensionMember::Getter { key, .. } => {
                    (ExtensionKind::Getter, interner.resolve(*key))
                }
                ExtensionMember::Setter { key, .. } => {
                    (ExtensionKind::Setter, interner.resolve(*key))
                }
            };
            ExtensionName {
                kind,
                member: Arc::from(member),
                mangled: Arc::from(kind.mangled(label, member)),
            }
        })
        .collect()
}
