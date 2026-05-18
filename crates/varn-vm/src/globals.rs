use crate::heap::Heap;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[derive(Clone)]
pub struct GlobalStore {
    pub values: Vec<VmValue>,
    names: FxHashMap<Rc<str>, usize>,
}

impl GlobalStore {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            names: FxHashMap::default(),
        }
    }

    pub fn with_native_layout(heap: &mut Heap) -> Self {
        let native_map = varn_builtins::dispatch::register_globals_vm(heap);

        let mut values = Vec::new();
        let mut names = FxHashMap::default();

        let prioritized = ["print", "assert"];
        let mut remaining: FxHashMap<Rc<str>, VmValue> = native_map;
        for &name in &prioritized {
            if let Some(val) = remaining.remove(name) {
                let idx = values.len();
                names.insert(Rc::from(name), idx);
                values.push(val);
            }
        }

        let mut entries: Vec<(Rc<str>, VmValue)> = remaining.into_iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

        for (name, val) in entries {
            names.insert(name, values.len());
            values.push(val);
        }

        Self { values, names }
    }

    pub fn define(&mut self, name: &str, value: VmValue) -> usize {
        if let Some(&idx) = self.names.get(name) {
            self.values[idx] = value;
            return idx;
        }
        let idx = self.values.len();
        self.values.push(value);
        self.names.insert(Rc::from(name), idx);
        idx
    }

    pub fn set_by_name(&mut self, name: &str, value: VmValue) -> bool {
        if let Some(&idx) = self.names.get(name) {
            self.values[idx] = value;
            return true;
        }
        false
    }

    #[inline(always)]
    pub fn set_by_index(&mut self, idx: usize, value: VmValue) {
        if idx >= self.values.len() {
            self.values.resize(idx + 1, VmValue::null());
        }
        self.values[idx] = value;
    }

    pub fn get_by_name(&self, name: &str) -> Option<VmValue> {
        let idx = *self.names.get(name)?;
        Some(self.values[idx])
    }

    #[inline(always)]
    pub fn get_by_index(&self, idx: usize) -> Option<VmValue> {
        self.values.get(idx).copied()
    }

    #[inline(always)]
    pub fn get_by_index_unchecked(&self, idx: usize) -> VmValue {
        self.values[idx]
    }

    #[inline(always)]
    pub fn set_by_index_unchecked(&mut self, idx: usize, value: VmValue) {
        self.values[idx] = value;
    }

    pub fn resolve_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
}

impl Default for GlobalStore {
    fn default() -> Self {
        Self::new()
    }
}
