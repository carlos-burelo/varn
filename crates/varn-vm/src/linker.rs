use rustc_hash::FxHashMap;
use varn_core::ModuleId;

use crate::value::VmValue;

/// State of a module in the link graph.
#[derive(Clone, Debug)]
pub enum ModuleLinkState {
    /// Module evaluation started; its frame is on the call stack.
    /// Any re-entry attempt signals a circular dependency.
    Evaluating,
    /// Module fully evaluated and its namespace value is cached.
    Done(VmValue),
}

/// Tracks per-module link state to detect cycles and cache results without
/// scanning the call-stack frames. Replaces the ad-hoc frame-position scan in
/// `ctx_modules::load_module`.
pub struct Linker {
    state: FxHashMap<ModuleId, ModuleLinkState>,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub fn new() -> Self {
        Self {
            state: FxHashMap::default(),
        }
    }

    /// Returns the cached result for `id`, if evaluation is complete.
    pub fn cached(&self, id: &ModuleId) -> Option<VmValue> {
        match self.state.get(id) {
            Some(ModuleLinkState::Done(v)) => Some(*v),
            _ => None,
        }
    }

    /// Returns true if `id` is currently being evaluated (cycle detection).
    pub fn is_evaluating(&self, id: &ModuleId) -> bool {
        matches!(self.state.get(id), Some(ModuleLinkState::Evaluating))
    }

    /// Mark `id` as evaluation-in-progress. Must call `set_done` when complete.
    pub fn set_evaluating(&mut self, id: ModuleId) {
        self.state.insert(id, ModuleLinkState::Evaluating);
    }

    /// Mark `id` as complete, caching its namespace value.
    pub fn set_done(&mut self, id: ModuleId, val: VmValue) {
        self.state.insert(id, ModuleLinkState::Done(val));
    }

    /// Remove an in-progress evaluating entry (for rollback after load failure).
    pub fn cancel_evaluating(&mut self, id: &ModuleId) {
        if matches!(self.state.get(id), Some(ModuleLinkState::Evaluating)) {
            self.state.remove(id);
        }
    }

    pub fn clone_state(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}
