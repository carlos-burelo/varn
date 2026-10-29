use super::graph::ModuleGraph;
use std::sync::Arc;
use varn_core::ModuleId;

pub struct ModuleResolver {
    pub(super) loader: varn_modules::loader::ModuleRegistry,
    pub(super) graph: parking_lot::Mutex<ModuleGraph>,

    pub(super) in_flight: parking_lot::Mutex<rustc_hash::FxHashSet<String>>,
    binding: parking_lot::ReentrantMutex<()>,

    pub(super) core_exports: parking_lot::Mutex<Option<Arc<varn_sem::cores::CoreExports>>>,
    pub(super) core_members: parking_lot::Mutex<Option<Arc<varn_sem::cores::CoreMembers>>>,
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self::with_registry(varn_modules::loader::default_registry())
    }

    pub fn with_registry(loader: varn_modules::loader::ModuleRegistry) -> Self {
        Self {
            loader,
            graph: parking_lot::Mutex::default(),
            in_flight: parking_lot::Mutex::default(),
            binding: parking_lot::ReentrantMutex::new(()),
            core_exports: parking_lot::Mutex::default(),
            core_members: parking_lot::Mutex::default(),
        }
    }

    pub(super) fn bind_guard(&self) -> parking_lot::ReentrantMutexGuard<'_, ()> {
        self.binding.lock()
    }

    pub(super) fn load_source(&self, id: &ModuleId) -> Option<varn_modules::loader::ModuleSource> {
        use varn_modules::loader::ModuleLoader;
        self.loader.source(id).ok()
    }

    pub fn invalidate(&self, id: &ModuleId) {
        self.graph.lock().invalidate(id);
    }

    pub fn clear(&self) {
        self.graph.lock().clear();
        *self.core_exports.lock() = None;
        *self.core_members.lock() = None;
    }

    pub fn types_cache_dir(&self) -> std::path::PathBuf {
        varn_modules::artifact::get_types_cache_dir()
    }
}
