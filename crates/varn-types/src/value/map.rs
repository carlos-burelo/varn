use crate::vm_value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;

pub const SMALL_MAP_CAP: usize = 8;

type Entry = (MapKey, VmValue);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapKey(pub VmValue);

impl std::hash::Hash for MapKey {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[derive(Clone, Debug)]
pub enum ValueMap {
    Small(Vec<Entry>),
    Large(rustc_hash::FxHashMap<MapKey, VmValue>),
}

impl Default for ValueMap {
    #[inline(always)]
    fn default() -> Self {
        Self::Small(Vec::new())
    }
}

impl ValueMap {
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn with_capacity(cap: usize) -> Self {
        if cap <= SMALL_MAP_CAP {
            Self::Small(Vec::with_capacity(cap))
        } else {
            Self::Large(rustc_hash::FxHashMap::with_capacity_and_hasher(
                cap,
                Default::default(),
            ))
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            Self::Small(entries) => entries.len(),
            Self::Large(map) => map.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub fn get(&self, key: &MapKey) -> Option<&VmValue> {
        match self {
            Self::Small(entries) => entries.iter().find(|e| e.0 == *key).map(|e| &e.1),
            Self::Large(map) => map.get(key),
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, key: &MapKey) -> Option<&mut VmValue> {
        match self {
            Self::Small(entries) => entries.iter_mut().find(|e| e.0 == *key).map(|e| &mut e.1),
            Self::Large(map) => map.get_mut(key),
        }
    }

    #[inline(always)]
    pub fn contains_key(&self, key: &MapKey) -> bool {
        self.get(key).is_some()
    }

    #[inline(always)]
    pub fn insert(&mut self, key: MapKey, val: VmValue) -> Option<VmValue> {
        match self {
            Self::Small(entries) => {
                if let Some(entry) = entries.iter_mut().find(|e| e.0 == key) {
                    return Some(std::mem::replace(&mut entry.1, val));
                }
                if entries.len() < SMALL_MAP_CAP {
                    entries.push((key, val));
                    return None;
                }
                let mut map = rustc_hash::FxHashMap::with_capacity_and_hasher(
                    SMALL_MAP_CAP * 2,
                    Default::default(),
                );
                map.extend(entries.drain(..));
                map.insert(key, val);
                *self = Self::Large(map);
                None
            }
            Self::Large(map) => map.insert(key, val),
        }
    }

    #[inline(always)]
    pub fn remove(&mut self, key: &MapKey) -> Option<VmValue> {
        match self {
            Self::Small(entries) => {
                let i = entries.iter().position(|e| e.0 == *key)?;
                Some(entries.swap_remove(i).1)
            }
            Self::Large(map) => map.remove(key),
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        match self {
            Self::Small(entries) => entries.clear(),
            Self::Large(map) => map.clear(),
        }
    }

    #[inline(always)]
    pub fn iter(&self) -> ValueMapIter<'_> {
        match self {
            Self::Small(entries) => ValueMapIter::Small(entries.iter()),
            Self::Large(map) => ValueMapIter::Large(map.iter()),
        }
    }

    #[inline(always)]
    pub fn keys(&self) -> ValueMapKeys<'_> {
        match self {
            Self::Small(entries) => ValueMapKeys::Small(entries.iter()),
            Self::Large(map) => ValueMapKeys::Large(map.keys()),
        }
    }

    #[inline(always)]
    pub fn values(&self) -> ValueMapValues<'_> {
        match self {
            Self::Small(entries) => ValueMapValues::Small(entries.iter()),
            Self::Large(map) => ValueMapValues::Large(map.values()),
        }
    }

    #[inline(always)]
    pub fn values_mut(&mut self) -> ValueMapValuesMut<'_> {
        match self {
            Self::Small(entries) => ValueMapValuesMut::Small(entries.iter_mut()),
            Self::Large(map) => ValueMapValuesMut::Large(map.values_mut()),
        }
    }

    #[inline(always)]
    pub fn drain(&mut self) -> ValueMapDrain<'_> {
        match self {
            Self::Small(entries) => ValueMapDrain::Small(entries.drain(..)),
            Self::Large(map) => ValueMapDrain::Large(map.drain()),
        }
    }
}

impl PartialEq for ValueMap {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (k, v) in self.iter() {
            if other.get(k) != Some(v) {
                return false;
            }
        }
        true
    }
}

impl Eq for ValueMap {}

macro_rules! dual_iter {
    ($name:ident, $small:ty, $large:ty, $item:ty, $map:expr) => {
        pub enum $name<'a> {
            Small($small),
            Large($large),
        }

        impl<'a> Iterator for $name<'a> {
            type Item = $item;

            #[inline(always)]
            fn next(&mut self) -> Option<Self::Item> {
                match self {
                    Self::Small(it) => it.next().map($map),
                    Self::Large(it) => it.next(),
                }
            }

            #[inline(always)]
            fn size_hint(&self) -> (usize, Option<usize>) {
                match self {
                    Self::Small(it) => it.size_hint(),
                    Self::Large(it) => it.size_hint(),
                }
            }
        }

        impl<'a> ExactSizeIterator for $name<'a> {}
    };
}

dual_iter!(
    ValueMapIter,
    std::slice::Iter<'a, Entry>,
    std::collections::hash_map::Iter<'a, MapKey, VmValue>,
    (&'a MapKey, &'a VmValue),
    |(k, v): &'a Entry| (k, v)
);
dual_iter!(
    ValueMapKeys,
    std::slice::Iter<'a, Entry>,
    std::collections::hash_map::Keys<'a, MapKey, VmValue>,
    &'a MapKey,
    |(k, _): &'a Entry| k
);
dual_iter!(
    ValueMapValues,
    std::slice::Iter<'a, Entry>,
    std::collections::hash_map::Values<'a, MapKey, VmValue>,
    &'a VmValue,
    |(_, v): &'a Entry| v
);
dual_iter!(
    ValueMapValuesMut,
    std::slice::IterMut<'a, Entry>,
    std::collections::hash_map::ValuesMut<'a, MapKey, VmValue>,
    &'a mut VmValue,
    |(_, v): &'a mut Entry| v
);
dual_iter!(
    ValueMapDrain,
    std::vec::Drain<'a, Entry>,
    std::collections::hash_map::Drain<'a, MapKey, VmValue>,
    Entry,
    |e: Entry| e
);

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
