//! The one keyed table behind `Map` and `Set`: entries in insertion order,
//! looked up linearly while small and through a position index once large.
//! Iteration order is the order keys were first inserted — never the order of
//! their bit patterns, which for heap keys differs from run to run.

use super::map::MapKey;
use crate::vm_value::VmValue;
use rustc_hash::FxHashMap;

const LINEAR_MAX: usize = 8;
const TOMBSTONE: MapKey = MapKey(VmValue::from_raw_parts(u64::MAX, 1));

#[derive(Clone, Debug)]
pub struct OrderedTable<V> {
    entries: Vec<(MapKey, V)>,
    index: Option<FxHashMap<MapKey, u32>>,
    live: u32,
}

impl<V> Default for OrderedTable<V> {
    #[inline(always)]
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            live: 0,
        }
    }
}

impl<V> OrderedTable<V> {
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        let mut t = Self {
            entries: Vec::with_capacity(cap),
            index: None,
            live: 0,
        };
        if cap > LINEAR_MAX {
            t.index = Some(FxHashMap::with_capacity_and_hasher(cap, Default::default()));
        }
        t
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.live as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    #[inline]
    fn position(&self, key: &MapKey) -> Option<usize> {
        match &self.index {
            Some(index) => index.get(key).map(|&p| p as usize),
            None => self.entries.iter().position(|e| e.0 == *key),
        }
    }

    #[inline]
    pub fn get(&self, key: &MapKey) -> Option<&V> {
        self.position(key).map(|p| &self.entries[p].1)
    }

    #[inline]
    pub fn get_mut(&mut self, key: &MapKey) -> Option<&mut V> {
        self.position(key).map(|p| &mut self.entries[p].1)
    }

    #[inline]
    pub fn contains_key(&self, key: &MapKey) -> bool {
        self.position(key).is_some()
    }

    pub fn insert(&mut self, key: MapKey, val: V) -> Option<V> {
        if let Some(p) = self.position(&key) {
            return Some(std::mem::replace(&mut self.entries[p].1, val));
        }
        let pos = self.entries.len() as u32;
        self.entries.push((key, val));
        self.live += 1;
        match &mut self.index {
            Some(index) => {
                index.insert(key, pos);
            }
            None if self.entries.len() > LINEAR_MAX => self.rebuild_index(),
            None => {}
        }
        None
    }

    pub fn remove(&mut self, key: &MapKey) -> Option<V>
    where
        V: Copy,
    {
        let p = self.position(key)?;
        if let Some(index) = &mut self.index {
            index.remove(key);
        }
        let val = self.entries[p].1;
        self.entries[p].0 = TOMBSTONE;
        self.live -= 1;
        let dead = self.entries.len() - self.live as usize;
        if dead > LINEAR_MAX && dead > self.live as usize {
            self.compact();
        }
        Some(val)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index = None;
        self.live = 0;
    }

    fn compact(&mut self) {
        self.entries.retain(|e| e.0 != TOMBSTONE);
        if self.index.is_some() || self.entries.len() > LINEAR_MAX {
            self.rebuild_index();
        }
    }

    fn rebuild_index(&mut self) {
        let mut index = FxHashMap::with_capacity_and_hasher(self.entries.len(), Default::default());
        for (pos, e) in self.entries.iter().enumerate() {
            if e.0 != TOMBSTONE {
                index.insert(e.0, pos as u32);
            }
        }
        self.index = Some(index);
    }

    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&MapKey, &V)> + '_ {
        Live {
            inner: self.entries.iter(),
            left: self.live as usize,
        }
        .map(|e| (&e.0, &e.1))
    }

    #[inline]
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &MapKey> + '_ {
        self.iter().map(|(k, _)| k)
    }

    #[inline]
    pub fn values(&self) -> impl ExactSizeIterator<Item = &V> + '_ {
        self.iter().map(|(_, v)| v)
    }

    #[inline]
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.entries
            .iter_mut()
            .filter(|e| e.0 != TOMBSTONE)
            .map(|e| &mut e.1)
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (MapKey, V)> + '_ {
        self.index = None;
        self.live = 0;
        self.entries.drain(..).filter(|e| e.0 != TOMBSTONE)
    }
}

struct Live<'a, V> {
    inner: std::slice::Iter<'a, (MapKey, V)>,
    left: usize,
}

impl<'a, V> Iterator for Live<'a, V> {
    type Item = &'a (MapKey, V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let e = self.inner.find(|e| e.0 != TOMBSTONE)?;
        self.left -= 1;
        Some(e)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<'a, V> ExactSizeIterator for Live<'a, V> {}

impl<V: PartialEq> PartialEq for OrderedTable<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl<V: Eq> Eq for OrderedTable<V> {}
