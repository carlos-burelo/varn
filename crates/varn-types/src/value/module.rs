use crate::native::NativeFn;
use crate::vm_value::VmValue;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::ModuleId;

#[derive(Debug, Clone)]
pub struct ModuleObj {
    pub id: ModuleId,
    pub exports: Vec<VmValue>,
    pub export_map: FxHashMap<Rc<str>, usize>,
}

#[derive(Debug, Clone)]
pub struct FrozenModuleObj {
    pub id: ModuleId,
    pub exports: Vec<FrozenExport>,
    pub export_map: FxHashMap<Arc<str>, usize>,
}

#[derive(Debug, Clone)]
pub enum FrozenExport {
    Primitive(VmValue),
    Str(Arc<str>),
    NativeFn(NativeFn, &'static str),
    Nested(Arc<FrozenModuleObj>),
}

impl FrozenModuleObj {
    pub fn new(id: ModuleId) -> Self {
        Self {
            id,
            exports: Vec::new(),
            export_map: FxHashMap::default(),
        }
    }

    pub fn push(&mut self, name: Arc<str>, export: FrozenExport) {
        let idx = self.exports.len();
        self.export_map.insert(name, idx);
        self.exports.push(export);
    }
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
