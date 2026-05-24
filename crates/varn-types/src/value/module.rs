use std::rc::Rc;
use rustc_hash::FxHashMap;
use crate::vm_value::VmValue;
use varn_core::ModuleId;

#[derive(Debug, Clone)]
pub struct ModuleObj {
    pub id: ModuleId,
    pub exports: Vec<VmValue>,
    pub export_map: FxHashMap<Rc<str>, usize>,
}

impl ModuleObj {
    pub fn new(id: ModuleId, size: usize) -> Self {
        Self {
            id,
            exports: vec![VmValue::null(); size],
            export_map: FxHashMap::default(),
        }
    }

    #[inline(always)]
    pub fn get_slot(&self, slot: usize) -> Option<VmValue> {
        self.exports.get(slot).copied()
    }

    #[inline(always)]
    pub fn set_slot(&mut self, slot: usize, val: VmValue) {
        if slot < self.exports.len() {
            self.exports[slot] = val;
        }
    }
}
