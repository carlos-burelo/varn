use super::resolver_disk::DiskResolver;
use super::ExportMap;
use crate::binder::BindResult;
use std::sync::Arc;

impl DiskResolver {
    pub(super) fn cached_bind(&self, key: &str) -> Option<Arc<BindResult>> {
        self.graph.lock().bind(key)
    }

    pub(super) fn store_bind(&self, key: String, bind: Arc<BindResult>) {
        self.graph.lock().insert_bind(key, bind);
    }

    pub(super) fn cached_exports(&self, key: &str) -> Option<Arc<ExportMap>> {
        self.graph.lock().exports(key)
    }

    pub(super) fn store_exports(&self, key: String, exports: Arc<ExportMap>) {
        self.graph.lock().insert_exports(key, exports);
    }

    pub(super) fn cached_program(&self, key: &str) -> Option<Arc<varn_core::ast::Program>> {
        self.graph.lock().program(key)
    }

    pub(super) fn store_program(&self, key: String, program: Arc<varn_core::ast::Program>) {
        self.graph.lock().insert_program(key, program);
    }

    pub(super) fn cached_arena(&self, key: &str) -> Option<Arc<varn_core::ast::AstArena>> {
        self.graph.lock().arena(key)
    }

    pub(super) fn store_arena(&self, key: String, arena: Arc<varn_core::ast::AstArena>) {
        self.graph.lock().insert_arena(key, arena);
    }

    pub(super) fn cached_path(&self, base_dir: &str, specifier: &str) -> Option<String> {
        self.graph.lock().resolved_path(base_dir, specifier)
    }

    pub(super) fn store_path(&self, base_dir: String, specifier: String, abs: String) {
        self.graph
            .lock()
            .insert_resolved_path(base_dir, specifier, abs);
    }
}
