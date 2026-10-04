use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub type RuntimeString = Arc<str>;

pub use super::map::{MapKey, MapRef, ValueMap, ValueSet};

/// Handle to a property object. The fields live inside this same allocation
/// (see `ObjData`), so there is no inner `RefCell` and no second buffer:
/// mutation goes through `ObjData`'s cells on `&self`.
#[derive(Clone)]
pub struct ObjRef(pub Rc<super::ObjData>);

impl ObjRef {
    /// Empty object on the root shape.
    pub fn empty() -> Self {
        Self(super::ObjData::new())
    }

    pub fn instance(class: &super::ClassObj) -> Self {
        Self(super::ObjData::new_instance(class))
    }

    pub fn instance_rc(class: Rc<super::ClassObj>) -> Self {
        Self(super::ObjData::new_instance(&class))
    }

    pub fn with_shape(shape: Rc<super::Shape>, values: Vec<crate::vm_value::VmValue>) -> Self {
        Self(super::ObjData::with_shape(shape, values))
    }

    /// As [`Self::with_shape`], from a borrowed buffer — see
    /// [`super::ObjData::with_shape_slice`].
    pub fn with_shape_slice(shape: Rc<super::Shape>, values: &[crate::vm_value::VmValue]) -> Self {
        Self(super::ObjData::with_shape_slice(shape, values))
    }

    pub fn from_pairs<I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (RuntimeString, crate::vm_value::VmValue)>,
    {
        Self(super::ObjData::from_pairs(pairs))
    }

    pub fn read(&self) -> &super::ObjData {
        &self.0
    }

    pub fn borrow(&self) -> &super::ObjData {
        &self.0
    }
}

impl std::ops::Deref for ObjRef {
    type Target = super::ObjData;
    #[inline(always)]
    fn deref(&self) -> &super::ObjData {
        &self.0
    }
}

impl PartialEq for ObjRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ObjRef {}

impl std::hash::Hash for ObjRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.0) as *const u8).hash(state);
    }
}

impl std::fmt::Debug for ObjRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ObjRef({:p})", Rc::as_ptr(&self.0) as *const u8)
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
