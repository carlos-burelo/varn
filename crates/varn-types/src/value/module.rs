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

/// A heap-index-free snapshot of a pure module's exports.
/// Created once at init time; thawed into each VM run's heap on import.
#[derive(Debug, Clone)]
pub struct FrozenModuleObj {
    pub id: ModuleId,
    pub exports: Vec<FrozenExport>,
    pub export_map: FxHashMap<Arc<str>, usize>,
}

/// Portable export value — no heap indices, safe to share across VM instances.
#[derive(Debug, Clone)]
pub enum FrozenExport {
    /// NaN-boxed primitive (int/float/bool/null) — copy-safe.
    Primitive(VmValue),
    /// Interned string — shared immutably.
    Str(Arc<str>),
    /// Native function pointer — stateless, shareable.
    NativeFn(NativeFn, &'static str),
    /// Nested frozen namespace (e.g. Math.Trig sub-object).
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
