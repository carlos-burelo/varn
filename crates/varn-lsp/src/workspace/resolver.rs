use std::sync::{OnceLock, RwLock};
use varn_checker::module_resolver::DiskResolver;

fn global_resolver() -> &'static RwLock<DiskResolver> {
    static RESOLVER: OnceLock<RwLock<DiskResolver>> = OnceLock::new();
    RESOLVER.get_or_init(|| RwLock::new(DiskResolver::new()))
}

pub fn with_resolver<R>(f: impl FnOnce(&DiskResolver) -> R) -> R {
    let guard = global_resolver().read().unwrap_or_else(|e| e.into_inner());
    f(&guard)
}

pub fn reset() {
    if let Ok(guard) = global_resolver().write() {
        guard.clear();
    }
    varn_core::clear_interner();
}

pub fn invalidate(id: &varn_core::ModuleId) {
    if let Ok(guard) = global_resolver().read() {
        guard.invalidate(id);
    }
}
