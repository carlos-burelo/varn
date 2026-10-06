use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use std::path::Path;
use std::sync::Arc;

pub trait ImportResolver {
    
    fn module_bind(&self, abs_path: &str) -> Option<Arc<BindResult>>;

    
    
    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Arc<ExportMap>;

    
    fn stdlib_bind(&self, specifier: &str) -> Option<Arc<BindResult>>;

    
    fn stdlib_exports(&self, specifier: &str) -> Arc<ExportMap>;

    
    fn resolve_specifier(&self, base_dir: &Path, specifier: &str) -> Option<String>;

    
    
    fn record_dep(&self, importer: &str, imported: &str);

    
    
    
    fn core_exports(&self) -> Arc<crate::core::loader::CoreExports>;

    
    
    
    
    
    
    
    
    fn evict_heavy(&self) -> (usize, usize, usize);

    
    
    
    fn graph_stats(&self) -> (usize, usize, usize, usize);

    
    fn core_members(&self) -> Arc<crate::core::loader::CoreMembers>;

    
    
    
    
    
    
    
    fn find_bind_for_type(
        &self,
        type_name: &str,
        origin_modules: &[String],
    ) -> Option<Arc<BindResult>> {
        for path in origin_modules {
            let Some(bind) = self.module_bind(path).or_else(|| self.stdlib_bind(path)) else {
                continue;
            };
            if bind.get_class_entry(type_name).is_some()
                || bind.get_namespace_members_local(type_name).is_some()
                || bind.get_interface_members_local(type_name).is_some()
            {
                return Some(bind);
            }
        }
        None
    }
}
