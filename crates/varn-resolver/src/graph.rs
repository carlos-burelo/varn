use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_sem::bind::BindResult;
use varn_sem::exports::ExportMap;

fn is_image_module(key: &str) -> bool {
    varn_core::ModuleKind::Core.owns(key) || varn_core::ModuleKind::Stdlib.owns(key)
}

#[derive(Default)]
pub struct ModuleGraph {
    binds: FxHashMap<String, Arc<BindResult>>,
    exports: FxHashMap<String, Arc<ExportMap>>,
    programs: FxHashMap<String, Arc<varn_core::ast::Program>>,

    arenas: FxHashMap<String, Arc<varn_core::ast::AstArena>>,

    resolved_paths: FxHashMap<(String, String), String>,

    reverse_deps: FxHashMap<String, Vec<String>>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&self, key: &str) -> Option<Arc<BindResult>> {
        self.binds.get(key).map(Arc::clone)
    }

    pub fn insert_bind(&mut self, key: String, bind: Arc<BindResult>) {
        self.binds.entry(key).or_insert(bind);
    }

    pub fn replace_bind(&mut self, key: String, bind: Arc<BindResult>) {
        self.binds.insert(key, bind);
    }

    pub fn exports(&self, key: &str) -> Option<Arc<ExportMap>> {
        self.exports.get(key).map(Arc::clone)
    }

    pub fn insert_exports(&mut self, key: String, exports: Arc<ExportMap>) {
        self.exports.insert(key, exports);
    }

    pub fn program(&self, key: &str) -> Option<Arc<varn_core::ast::Program>> {
        self.programs.get(key).map(Arc::clone)
    }

    pub fn insert_program(&mut self, key: String, program: Arc<varn_core::ast::Program>) {
        self.programs.entry(key).or_insert(program);
    }

    pub fn arena(&self, key: &str) -> Option<Arc<varn_core::ast::AstArena>> {
        self.arenas.get(key).map(Arc::clone)
    }

    pub fn insert_arena(&mut self, key: String, arena: Arc<varn_core::ast::AstArena>) {
        self.arenas.entry(key).or_insert(arena);
    }

    pub fn resolved_path(&self, base_dir: &str, specifier: &str) -> Option<String> {
        self.resolved_paths
            .get(&(base_dir.to_owned(), specifier.to_owned()))
            .cloned()
    }

    pub fn insert_resolved_path(&mut self, base_dir: String, specifier: String, abs: String) {
        self.resolved_paths.insert((base_dir, specifier), abs);
    }

    pub fn record_dep(&mut self, importer: &str, imported: &str) {
        self.reverse_deps
            .entry(imported.to_owned())
            .or_default()
            .push(importer.to_owned());
    }

    pub fn invalidate(&mut self, id: &ModuleId) {
        let key = id.as_str();
        let mut to_clear = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut queue = vec![key.clone()];

        while let Some(k) = queue.pop() {
            if !visited.insert(k.clone()) {
                continue;
            }
            if let Some(deps) = self.reverse_deps.get(&k) {
                queue.extend(deps.iter().cloned());
            }
            to_clear.push(k);
        }

        for k in &to_clear {
            self.binds.remove(k);
            self.exports.remove(k);
            self.programs.remove(k);
            self.arenas.remove(k);
        }
        self.resolved_paths.retain(|_, v| !to_clear.contains(v));
    }

    pub fn evict_heavy(&mut self) -> (usize, usize, usize) {
        let binds_before = self.binds.len();
        self.binds.retain(|key, _| is_image_module(key));
        let counts = (
            binds_before - self.binds.len(),
            self.programs.len(),
            self.arenas.len(),
        );
        self.programs.clear();
        self.arenas.clear();
        counts
    }

    pub fn heavy_stats(&self) -> (usize, usize, usize, usize) {
        (
            self.binds.len(),
            self.programs.len(),
            self.arenas.len(),
            self.exports.len(),
        )
    }

    pub fn clear(&mut self) {
        self.binds.clear();
        self.exports.clear();
        self.programs.clear();
        self.arenas.clear();
        self.resolved_paths.clear();
        self.reverse_deps.clear();
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn module_graph_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<super::ModuleGraph>();
    }
}
