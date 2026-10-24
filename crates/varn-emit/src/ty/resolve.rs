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

pub struct ScopedTypeParams<'a> {
    pub inner: &'a dyn NameResolver,
    pub params: &'a [std::sync::Arc<str>],
}

impl NameResolver for ScopedTypeParams<'_> {
    fn class_id(&self, name: &str) -> Option<ClassId> {
        self.inner.class_id(name)
    }
    fn enum_id(&self, name: &str) -> Option<EnumId> {
        self.inner.enum_id(name)
    }
    fn foreign_enum_id(&self, name: &str, origin: &str) -> Option<EnumId> {
        self.inner.foreign_enum_id(name, origin)
    }
    fn foreign_class_id(&self, name: &str, origin: &str) -> Option<ClassId> {
        self.inner.foreign_class_id(name, origin)
    }
    fn is_local_origin(&self, origin: &str) -> bool {
        self.inner.is_local_origin(origin)
    }
    fn alias(&self, name: &str) -> Option<varn_sem::types::Type> {
        self.inner.alias(name)
    }
    fn is_type_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.as_ref() == name) || self.inner.is_type_param(name)
    }
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
