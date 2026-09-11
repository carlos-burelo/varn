use std::cell::RefCell;
use std::rc::Rc;
use crate::vm_value::VmValue;

pub const SMALL_MAP_CAP: usize = 8;

const EMPTY_ENTRY: (MapKey, VmValue) = (MapKey(VmValue::null()), VmValue::null());

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
    Small {
        len: u8,
        entries: [(MapKey, VmValue); SMALL_MAP_CAP],
    },
    Large(rustc_hash::FxHashMap<MapKey, VmValue>),
}

impl Default for ValueMap {
    #[inline(always)]
    fn default() -> Self {
        Self::Small {
            len: 0,
            entries: [EMPTY_ENTRY; SMALL_MAP_CAP],
        }
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
            Self::default()
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
            Self::Small { len, .. } => *len as usize,
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
            Self::Small { len, entries } => {
                let n = *len as usize;
                for i in 0..n {
                    if entries[i].0 == *key {
                        return Some(&entries[i].1);
                    }
                }
                None
            }
            Self::Large(map) => map.get(key),
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, key: &MapKey) -> Option<&mut VmValue> {
        match self {
            Self::Small { len, entries } => {
                let n = *len as usize;
                for i in 0..n {
                    if entries[i].0 == *key {
                        return Some(&mut entries[i].1);
                    }
                }
                None
            }
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
            Self::Small { len, entries } => {
                let n = *len as usize;
                for i in 0..n {
                    if entries[i].0 == key {
                        let old = entries[i].1;
                        entries[i].1 = val;
                        return Some(old);
                    }
                }
                if n < SMALL_MAP_CAP {
                    entries[n] = (key, val);
                    *len += 1;
                    return None;
                }
                let mut map = rustc_hash::FxHashMap::with_capacity_and_hasher(
                    SMALL_MAP_CAP * 2,
                    Default::default(),
                );
                for i in 0..SMALL_MAP_CAP {
                    map.insert(entries[i].0, entries[i].1);
                }
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
            Self::Small { len, entries } => {
                let n = *len as usize;
                for i in 0..n {
                    if entries[i].0 == *key {
                        let old = entries[i].1;
                        if i + 1 < n {
                            entries[i] = entries[n - 1];
                        }
                        *len -= 1;
                        return Some(old);
                    }
                }
                None
            }
            Self::Large(map) => map.remove(key),
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        match self {
            Self::Small { len, .. } => *len = 0,
            Self::Large(map) => map.clear(),
        }
    }

    #[inline(always)]
    pub fn iter(&self) -> ValueMapIter<'_> {
        match self {
            Self::Small { len, entries } => ValueMapIter::Small {
                slice: entries[..(*len as usize)].iter(),
            },
            Self::Large(map) => ValueMapIter::Large(map.iter()),
        }
    }

    #[inline(always)]
    pub fn keys(&self) -> ValueMapKeys<'_> {
        match self {
            Self::Small { len, entries } => ValueMapKeys::Small {
                slice: entries[..(*len as usize)].iter(),
            },
            Self::Large(map) => ValueMapKeys::Large(map.keys()),
        }
    }

    #[inline(always)]
    pub fn values(&self) -> ValueMapValues<'_> {
        match self {
            Self::Small { len, entries } => ValueMapValues::Small {
                slice: entries[..(*len as usize)].iter(),
            },
            Self::Large(map) => ValueMapValues::Large(map.values()),
        }
    }

    #[inline(always)]
    pub fn values_mut(&mut self) -> ValueMapValuesMut<'_> {
        match self {
            Self::Small { len, entries } => ValueMapValuesMut::Small {
                slice: entries[..(*len as usize)].iter_mut(),
            },
            Self::Large(map) => ValueMapValuesMut::Large(map.values_mut()),
        }
    }

    #[inline(always)]
    pub fn drain(&mut self) -> ValueMapDrain<'_> {
        match self {
            Self::Small { len, entries } => {
                let n = *len as usize;
                *len = 0;
                ValueMapDrain::Small {
                    items: *entries,
                    idx: 0,
                    len: n,
                }
            }
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

pub enum ValueMapIter<'a> {
    Small {
        slice: std::slice::Iter<'a, (MapKey, VmValue)>,
    },
    Large(std::collections::hash_map::Iter<'a, MapKey, VmValue>),
}

impl<'a> Iterator for ValueMapIter<'a> {
    type Item = (&'a MapKey, &'a VmValue);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Small { slice } => slice.next().map(|(k, v)| (k, v)),
            Self::Large(iter) => iter.next(),
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Small { slice } => slice.size_hint(),
            Self::Large(iter) => iter.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ValueMapIter<'a> {}

pub enum ValueMapKeys<'a> {
    Small {
        slice: std::slice::Iter<'a, (MapKey, VmValue)>,
    },
    Large(std::collections::hash_map::Keys<'a, MapKey, VmValue>),
}

impl<'a> Iterator for ValueMapKeys<'a> {
    type Item = &'a MapKey;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Small { slice } => slice.next().map(|(k, _)| k),
            Self::Large(keys) => keys.next(),
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Small { slice } => slice.size_hint(),
            Self::Large(keys) => keys.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ValueMapKeys<'a> {}

pub enum ValueMapValues<'a> {
    Small {
        slice: std::slice::Iter<'a, (MapKey, VmValue)>,
    },
    Large(std::collections::hash_map::Values<'a, MapKey, VmValue>),
}

impl<'a> Iterator for ValueMapValues<'a> {
    type Item = &'a VmValue;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Small { slice } => slice.next().map(|(_, v)| v),
            Self::Large(vals) => vals.next(),
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Small { slice } => slice.size_hint(),
            Self::Large(vals) => vals.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ValueMapValues<'a> {}

pub enum ValueMapValuesMut<'a> {
    Small {
        slice: std::slice::IterMut<'a, (MapKey, VmValue)>,
    },
    Large(std::collections::hash_map::ValuesMut<'a, MapKey, VmValue>),
}

impl<'a> Iterator for ValueMapValuesMut<'a> {
    type Item = &'a mut VmValue;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Small { slice } => slice.next().map(|(_, v)| v),
            Self::Large(vals) => vals.next(),
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Small { slice } => slice.size_hint(),
            Self::Large(vals) => vals.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ValueMapValuesMut<'a> {}

pub enum ValueMapDrain<'a> {
    Small {
        items: [(MapKey, VmValue); SMALL_MAP_CAP],
        idx: usize,
        len: usize,
    },
    Large(std::collections::hash_map::Drain<'a, MapKey, VmValue>),
}

impl<'a> Iterator for ValueMapDrain<'a> {
    type Item = (MapKey, VmValue);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Small { items, idx, len } => {
                if *idx < *len {
                    let item = items[*idx];
                    *idx += 1;
                    Some(item)
                } else {
                    None
                }
            }
            Self::Large(drain) => drain.next(),
        }
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Small { idx, len, .. } => {
                let rem = len.saturating_sub(*idx);
                (rem, Some(rem))
            }
            Self::Large(drain) => drain.size_hint(),
        }
    }
}

impl<'a> ExactSizeIterator for ValueMapDrain<'a> {}

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
