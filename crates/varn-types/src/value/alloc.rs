use std::sync::Arc;

pub type RuntimeString = Arc<str>;

pub use super::map::{MapKey, MapRef, SetRef, ValueMap, ValueSet};

#[derive(Clone, Copy)]
pub struct ObjRef(pub(crate) std::ptr::NonNull<super::ObjData>);

impl ObjRef {
    pub fn read(&self) -> &super::ObjData {
        self
    }

    pub fn borrow(&self) -> &super::ObjData {
        self
    }
}

impl std::ops::Deref for ObjRef {
    type Target = super::ObjData;
    #[inline(always)]
    fn deref(&self) -> &super::ObjData {
        unsafe { self.0.as_ref() }
    }
}

impl PartialEq for ObjRef {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for ObjRef {}

impl std::hash::Hash for ObjRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as *const u8).hash(state);
    }
}

impl std::fmt::Debug for ObjRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObjRef({:p})", self.0.as_ptr() as *const u8)
    }
}
