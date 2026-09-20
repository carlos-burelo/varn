use crate::heap::Heap;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Clone)]
#[repr(C)]
pub struct GlobalStore {
    pub values: Vec<VmValue>,
    names: FxHashMap<Arc<str>, usize>,
    pub idx_to_name: Vec<Arc<str>>,
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
        // `native_global_layout()` is the authority on ORDER (and the checker
        // emits `LoadNativeGlobalIdx` against it); `register_globals_vm` only
        // supplies the values.
        let mut native_map = varn_builtins::register_globals_vm(heap);
        let order = varn_builtins::native_global_layout();

        let mut values = Vec::with_capacity(order.len());
        let mut names = FxHashMap::default();
        let mut idx_to_name: Vec<Arc<str>> = Vec::with_capacity(order.len());

        for &name in order {
            let rc_name: Arc<str> = Arc::from(name);
            let val = native_map.remove(name).unwrap_or(VmValue::null());
            names.insert(rc_name.clone(), values.len());
            idx_to_name.push(rc_name);
            values.push(val);
        }

        // Anything the value map carried that the layout did not name (should be
        // nothing) is appended sorted — it stays reachable by name, just not at
        // a compile-time-known index.
        let mut leftover: Vec<(Arc<str>, VmValue)> = native_map.into_iter().collect();
        leftover.sort_by(|(a, _), (b, _)| a.as_ref().cmp(b.as_ref()));
        for (name, val) in leftover {
            names.insert(name.clone(), values.len());
            idx_to_name.push(name);
            values.push(val);
        }

        Self {
            values,
            names,
            idx_to_name,
        }
    }

    /// Reserve a contiguous region of `count` fresh (null) slots for a module's
    /// globals and return its base index. `LoadGlobalIdx` / `StoreGlobalIdx`
    /// carry slots relative to this base; the running closure carries the base.
    ///
    /// The region is anonymous — `idx_to_name` gets placeholder entries so the
    /// two vecs stay the same length; a later `define` of the same name (a
    /// module top-level `DefineGlobal` for a name that also needs a string key,
    /// e.g. a re-export) still appends rather than reusing the slot, which is
    /// fine: the indexed ops never consult `idx_to_name`.
    pub(crate) fn reserve_region(&mut self, count: u32) -> u32 {
        let base = self.values.len() as u32;
        let empty: Arc<str> = Arc::from("");
        for _ in 0..count {
            self.values.push(VmValue::null());
            self.idx_to_name.push(empty.clone());
        }
        base
    }

    pub(crate) fn define(&mut self, name: &str, value: VmValue) -> usize {
        if let Some(&idx) = self.names.get(name) {
            self.values[idx] = value;
            return idx;
        }
        let idx = self.values.len();
        let rc_name: Arc<str> = Arc::from(name);
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
}

impl Default for GlobalStore {
    fn default() -> Self {
        Self::new()
    }
}
