use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub type RuntimeString = Arc<str>;

pub use super::map::{MapKey, MapRef, ValueMap, ValueSet};




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

#[derive(Clone)]
pub struct SetRef(pub Rc<RefCell<ValueSet>>);

impl SetRef {
    pub fn new(data: ValueSet) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, ValueSet> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, ValueSet> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, ValueSet> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, ValueSet> {
        self.0.borrow_mut()
    }
}

impl PartialEq for SetRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SetRef {}

impl std::hash::Hash for SetRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for SetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SetRef({:p})", self.0)
    }
}
