use crate::heap::Heap;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[derive(Clone)]
#[repr(C)]
pub struct GlobalStore {
    pub values: Vec<VmValue>,
    names: FxHashMap<Rc<str>, usize>,
    pub idx_to_name: Vec<Rc<str>>,
}

impl GlobalStore {
    pub(crate) fn new() -> Self {
        Self {
            values: Vec::new(),
            names: FxHashMap::default(),
            idx_to_name: Vec::new(),
        }
    }

    pub(crate) fn with_native_layout(heap: &mut Heap) -> Self {
        let native_map = varn_builtins::dispatch::register_globals_vm(heap);

        let mut values = Vec::new();
        let mut names = FxHashMap::default();
        let mut idx_to_name: Vec<Rc<str>> = Vec::new();

        let prioritized = ["print", "assert"];
        let mut remaining = native_map;
        for &name in &prioritized {
            if let Some(val) = remaining.remove(name) {
                let idx = values.len();
                let rc_name: Rc<str> = Rc::from(name);
                idx_to_name.push(rc_name.clone());
                names.insert(rc_name, idx);
                values.push(val);
            }
        }

        let mut entries: Vec<(Rc<str>, VmValue)> = remaining.into_iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));

        for (name, val) in entries {
            idx_to_name.push(name.clone());
            names.insert(name, values.len());
            values.push(val);
        }

        Self {
            values,
            names,
            idx_to_name,
        }
    }

    pub(crate) fn define(&mut self, name: &str, value: VmValue) -> usize {
        if let Some(&idx) = self.names.get(name) {
            self.values[idx] = value;
            return idx;
        }
        let idx = self.values.len();
        let rc_name: Rc<str> = Rc::from(name);
        self.idx_to_name.push(rc_name.clone());
        self.values.push(value);
        self.names.insert(rc_name, idx);
        idx
    }

    pub(crate) fn set_by_name(&mut self, name: &str, value: VmValue) -> bool {
        if let Some(&idx) = self.names.get(name) {
            self.values[idx] = value;
            return true;
        }
        false
    }

    #[inline(always)]
    pub(crate) fn set_by_index(&mut self, idx: usize, value: VmValue) {
        if idx >= self.values.len() {
            self.values.resize(idx + 1, VmValue::null());
        }
        self.values[idx] = value;
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<VmValue> {
        let idx = *self.names.get(name)?;
        Some(self.values[idx])
    }

    #[inline(always)]
    pub(crate) fn get_by_index(&self, idx: usize) -> Option<VmValue> {
        self.values.get(idx).copied()
    }

    #[inline(always)]
    pub(crate) fn get_by_index_unchecked(&self, idx: usize) -> VmValue {
        debug_assert!(
            idx < self.values.len(),
            "global index OOB: {idx} >= {}",
            self.values.len()
        );
        unsafe { *self.values.get_unchecked(idx) }
    }

    #[inline(always)]
    pub(crate) fn set_by_index_unchecked(&mut self, idx: usize, value: VmValue) {
        debug_assert!(
            idx < self.values.len(),
            "global index OOB: {idx} >= {}",
            self.values.len()
        );
        unsafe {
            *self.values.get_unchecked_mut(idx) = value;
        }
    }

    pub(crate) fn resolve_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }
}

impl Default for GlobalStore {
    fn default() -> Self {
        Self::new()
    }
}
