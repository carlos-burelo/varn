use super::ordered::OrderedTable;
use crate::vm_value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapKey(pub VmValue);

impl std::hash::Hash for MapKey {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

pub type ValueMap = OrderedTable<VmValue>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValueSet(OrderedTable<()>);

impl ValueSet {
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn contains(&self, key: &MapKey) -> bool {
        self.0.contains_key(key)
    }

    #[inline]
    pub fn insert(&mut self, key: MapKey) -> bool {
        self.0.insert(key, ()).is_none()
    }

    #[inline]
    pub fn remove(&mut self, key: &MapKey) -> bool {
        self.0.remove(key).is_some()
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &MapKey> + '_ {
        self.0.keys()
    }
}

#[derive(Clone)]
pub struct MapRef(pub Rc<RefCell<ValueMap>>);

impl MapRef {
    pub fn new(data: ValueMap) -> Self {
        Self(Rc::new(RefCell::new(data)))
    }

    pub fn read(&self) -> std::cell::Ref<'_, ValueMap> {
        self.0.borrow()
    }

    pub fn write(&self) -> std::cell::RefMut<'_, ValueMap> {
        self.0.borrow_mut()
    }

    pub fn borrow(&self) -> std::cell::Ref<'_, ValueMap> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, ValueMap> {
        self.0.borrow_mut()
    }
}

impl PartialEq for MapRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for MapRef {}

impl std::hash::Hash for MapRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl std::fmt::Debug for MapRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapRef({:p})", self.0)
    }
}
