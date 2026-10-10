use super::ordered::OrderedTable;
use crate::vm_value::VmValue;
use std::cell::UnsafeCell;
use std::ptr::NonNull;

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

pub struct CellTable<T>(NonNull<UnsafeCell<T>>);

pub type MapRef = CellTable<ValueMap>;
pub type SetRef = CellTable<ValueSet>;

impl<T> Clone for CellTable<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for CellTable<T> {}

impl<T> CellTable<T> {
    #[inline(always)]
    pub unsafe fn init_at(at: *mut T, table: T) -> Self {
        unsafe {
            std::ptr::write(at, table);
            Self(NonNull::new_unchecked(at as *mut UnsafeCell<T>))
        }
    }

    pub unsafe fn drop_at(self) {
        unsafe { std::ptr::drop_in_place(self.0.as_ptr()) };
    }

    #[inline(always)]
    pub fn read(&self) -> &T {
        unsafe { &*(*self.0.as_ptr()).get() }
    }

    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn write(&self) -> &mut T {
        unsafe { &mut *(*self.0.as_ptr()).get() }
    }
}

impl<T> PartialEq for CellTable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for CellTable<T> {}

impl<T> std::hash::Hash for CellTable<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T> std::fmt::Debug for CellTable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CellTable({:p})", self.0)
    }
}
