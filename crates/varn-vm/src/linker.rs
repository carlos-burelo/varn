use rustc_hash::FxHashMap;
use std::cell::UnsafeCell;
use std::rc::Rc;
use varn_core::ModuleId;

use crate::value::VmValue;

#[derive(Clone, Debug)]
pub enum ModuleLinkState {
    Evaluating,

    Done(VmValue),
}

pub struct Linker {
    state: Rc<UnsafeCell<FxHashMap<ModuleId, ModuleLinkState>>>,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(UnsafeCell::new(FxHashMap::default())),
        }
    }

    fn table(&self) -> &FxHashMap<ModuleId, ModuleLinkState> {
        unsafe { &*self.state.get() }
    }

    #[allow(clippy::mut_from_ref)]
    fn table_mut(&self) -> &mut FxHashMap<ModuleId, ModuleLinkState> {
        unsafe { &mut *self.state.get() }
    }

    pub(crate) fn cached(&self, id: &ModuleId) -> Option<VmValue> {
        match self.table().get(id) {
            Some(ModuleLinkState::Done(v)) => Some(*v),
            Some(ModuleLinkState::Evaluating) | None => None,
        }
    }

    pub(crate) fn is_evaluating(&self, id: &ModuleId) -> bool {
        matches!(self.table().get(id), Some(ModuleLinkState::Evaluating))
    }

    pub(crate) fn set_evaluating(&mut self, id: ModuleId) {
        self.table_mut().insert(id, ModuleLinkState::Evaluating);
    }

    pub(crate) fn set_done(&mut self, id: ModuleId, val: VmValue) {
        self.table_mut().insert(id, ModuleLinkState::Done(val));
    }

    pub(crate) fn cancel_evaluating(&mut self, id: &ModuleId) {
        if matches!(self.table().get(id), Some(ModuleLinkState::Evaluating)) {
            self.table_mut().remove(id);
        }
    }

    pub(crate) fn share(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
        }
    }
}
