





use super::BindResult;
use crate::types::{ClassMemberKind, Type};
use std::sync::Arc;



pub(crate) type VariantLayout = (Arc<str>, Vec<(Arc<str>, Type)>);

impl BindResult {
    
    pub(crate) fn declares_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.sum_type_variants.contains_key(name)
    }

    
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
