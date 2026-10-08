use super::graph::ModuleGraph;
use std::sync::Arc;
use varn_core::ModuleId;

pub struct DiskResolver {
    pub(super) loader: varn_modules::loader::ModuleRegistry,
    pub(super) graph: parking_lot::Mutex<ModuleGraph>,

    pub(super) in_flight: parking_lot::Mutex<rustc_hash::FxHashSet<String>>,

    pub(super) core_exports: parking_lot::Mutex<Option<Arc<varn_sem::cores::CoreExports>>>,
    pub(super) core_members: parking_lot::Mutex<Option<Arc<varn_sem::cores::CoreMembers>>>,
}

impl Default for DiskResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskResolver {
    pub fn new() -> Self {
        Self::with_registry(varn_modules::loader::default_registry())
    }

    pub fn with_registry(loader: varn_modules::loader::ModuleRegistry) -> Self {
        Self {
            loader,
            graph: parking_lot::Mutex::default(),
            in_flight: parking_lot::Mutex::default(),
            core_exports: parking_lot::Mutex::default(),
            core_members: parking_lot::Mutex::default(),
        }
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
        let root = self.graph.lock().project_root_or_init().clone();
        varn_modules::artifact::get_types_cache_dir(&root)
    }
}
