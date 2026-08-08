use rustc_hash::FxHashMap;
use varn_core::ModuleId;

use crate::value::VmValue;

#[derive(Clone, Debug)]
pub enum ModuleLinkState {
    Evaluating,

    Done(VmValue),
}

pub struct Linker {
    state: FxHashMap<ModuleId, ModuleLinkState>,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub(crate) fn new() -> Self {
        Self {
            state: FxHashMap::default(),
        }
    }

    pub(crate) fn cached(&self, id: &ModuleId) -> Option<VmValue> {
        match self.state.get(id) {
            Some(ModuleLinkState::Done(v)) => Some(*v),
            _ => None,
        }
    }

    pub(crate) fn is_evaluating(&self, id: &ModuleId) -> bool {
        matches!(self.state.get(id), Some(ModuleLinkState::Evaluating))
    }

    pub(crate) fn set_evaluating(&mut self, id: ModuleId) {
        self.state.insert(id, ModuleLinkState::Evaluating);
    }

    pub(crate) fn set_done(&mut self, id: ModuleId, val: VmValue) {
        self.state.insert(id, ModuleLinkState::Done(val));
    }

    pub(crate) fn cancel_evaluating(&mut self, id: &ModuleId) {
        if matches!(self.state.get(id), Some(ModuleLinkState::Evaluating)) {
            self.state.remove(id);
        }
    }

    pub(crate) fn clone_state(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
