use varn_tir::{ClassId, EnumId};

pub trait NameResolver {
    fn class_id(&self, name: &str) -> Option<ClassId>;
    fn enum_id(&self, name: &str) -> Option<EnumId>;
    fn foreign_enum_id(&self, name: &str, origin: &str) -> Option<EnumId>;
    fn foreign_class_id(&self, name: &str, origin: &str) -> Option<ClassId>;
    fn is_local_origin(&self, origin: &str) -> bool;
    fn alias(&self, name: &str) -> Option<varn_sem::types::Type>;
    fn is_type_param(&self, name: &str) -> bool;
}

pub struct NoNames;

impl NameResolver for NoNames {
    fn class_id(&self, _: &str) -> Option<ClassId> {
        None
    }
    fn enum_id(&self, _: &str) -> Option<EnumId> {
        None
    }
    fn foreign_enum_id(&self, _: &str, _: &str) -> Option<EnumId> {
        None
    }
    fn foreign_class_id(&self, _: &str, _: &str) -> Option<ClassId> {
        None
    }
    fn is_local_origin(&self, _: &str) -> bool {
        true
    }
    fn alias(&self, _: &str) -> Option<varn_sem::types::Type> {
        None
    }
    fn is_type_param(&self, _: &str) -> bool {
        false
    }
}
