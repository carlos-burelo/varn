use varn_tir::{ClassId, EnumId};

pub trait NameResolver {
    fn class_id(&self, name: &str) -> Option<ClassId>;
    fn enum_id(&self, name: &str) -> Option<EnumId>;
    fn foreign_enum_id(&self, name: &str, origin: &str) -> Option<EnumId>;
    fn is_local_origin(&self, origin: &str) -> bool;
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
    fn is_local_origin(&self, _: &str) -> bool {
        true
    }
}
