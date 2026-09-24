//! The types a module declares, and an enum's variants in tag order with
//! their payload fields. The tag of a variant is its position in
//! `enum_layout`: every module that builds or reads a value of the enum — the
//! declaring one and any importer — derives it from this one function, so
//! they agree by construction.

use super::BindResult;
use crate::types::{ClassMemberKind, Type};
use std::sync::Arc;

/// One variant: its name and its payload fields, typed in the declaring
/// module's table.
pub(crate) type VariantLayout = (Arc<str>, Vec<(Arc<str>, Type)>);

impl BindResult {
    /// Whether this module declares a type named `name`.
    pub(crate) fn declares_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.sum_type_variants.contains_key(name)
    }

    /// The variants of the enum `name` declared in this module, in tag order.
    pub(crate) fn enum_layout(&self, name: &str) -> Option<Vec<VariantLayout>> {
        let fields = |variant: &Arc<str>| {
            self.sum_variant_fields
                .get(variant)
                .cloned()
                .unwrap_or_default()
        };
        if let Some(variants) = self.sum_type_variants.get(name) {
            return Some(variants.iter().map(|v| (v.clone(), fields(v))).collect());
        }
        let members = self.type_members.enums.get(name)?;
        Some(
            members
                .iter()
                // Methods, accessors and `static` members share the enum body
                // but are not variants — including them shifts every tag.
                .filter(|m| {
                    !m.is_static
                        && !matches!(
                            m.kind,
                            ClassMemberKind::Method
                                | ClassMemberKind::Getter
                                | ClassMemberKind::Setter
                                | ClassMemberKind::Constructor
                        )
                })
                .map(|m| (m.name.clone(), fields(&m.name)))
                .collect(),
        )
    }
}
