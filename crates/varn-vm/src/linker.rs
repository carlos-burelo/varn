use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_core::ModuleId;

use crate::value::VmValue;

#[derive(Clone, Debug)]
pub enum ModuleLinkState {
    Evaluating,

    Done(VmValue),
}

pub struct Linker {
    state: Rc<RefCell<FxHashMap<ModuleId, ModuleLinkState>>>,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    pub(crate) fn cached(&self, id: &ModuleId) -> Option<VmValue> {
        match self.state.borrow().get(id) {
            Some(ModuleLinkState::Done(v)) => Some(*v),
            Some(ModuleLinkState::Evaluating) | None => None,
        }
    }

    pub(crate) fn is_evaluating(&self, id: &ModuleId) -> bool {
        matches!(
            self.state.borrow().get(id),
            Some(ModuleLinkState::Evaluating)
        )
    }

    pub(crate) fn set_evaluating(&mut self, id: ModuleId) {
        self.state
            .borrow_mut()
            .insert(id, ModuleLinkState::Evaluating);
    }

    pub(crate) fn set_done(&mut self, id: ModuleId, val: VmValue) {
        self.state
            .borrow_mut()
            .insert(id, ModuleLinkState::Done(val));
    }

    pub(crate) fn cancel_evaluating(&mut self, id: &ModuleId) {
        if matches!(
            self.state.borrow().get(id),
            Some(ModuleLinkState::Evaluating)
        ) {
            self.state.borrow_mut().remove(id);
        }
    }

    pub(crate) fn share(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
        }
    }
}
