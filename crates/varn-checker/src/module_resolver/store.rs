use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use varn_core::ModuleId;

thread_local! {
    pub(super) static PROJECT_ROOT: RefCell<Option<std::path::PathBuf>> = RefCell::new(None);
    pub(super) static MODULE_BIND_CACHE: RefCell<Option<FxHashMap<String, Rc<BindResult>>>> = RefCell::new(None);
    pub(super) static MODULE_EXPORT_CACHE: RefCell<Option<FxHashMap<String, Rc<ExportMap>>>> = RefCell::new(None);
    pub(super) static RESOLVED_PATH_CACHE: RefCell<Option<FxHashMap<(String, String), String>>> = RefCell::new(None);
    pub(super) static PROGRAM_CACHE: RefCell<Option<FxHashMap<String, Rc<varn_core::ast::Program>>>> = RefCell::new(None);
    pub(super) static REVERSE_DEPS: RefCell<FxHashMap<String, Vec<String>>> = RefCell::new(FxHashMap::default());
}

pub(super) fn export_cache_get(key: &str) -> Option<Rc<ExportMap>> {
    MODULE_EXPORT_CACHE.with(|c| {
        let guard = c.borrow();
        let cache = guard.as_ref()?;
        cache.get(key).map(Rc::clone)
    })
}

pub(super) fn export_cache_insert(key: String, exports: Rc<ExportMap>) {
    MODULE_EXPORT_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.insert(key, exports);
    });
}

pub(super) fn bind_cache_get(key: &str) -> Option<Rc<BindResult>> {
    MODULE_BIND_CACHE.with(|c| {
        let guard = c.borrow();
        let cache = guard.as_ref()?;
        cache.get(key).map(Rc::clone)
    })
}

pub(super) fn bind_cache_insert(key: String, bind: Rc<BindResult>) {
    MODULE_BIND_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.entry(key).or_insert(bind);
    });
}

pub(super) fn record_dep(importer: &str, imported: &str) {
    REVERSE_DEPS.with(|r| {
        r.borrow_mut()
            .entry(imported.to_owned())
            .or_default()
            .push(importer.to_owned());
    });
}

pub fn invalidate_module(id: &ModuleId) {
    let key = id.as_str();
    let mut to_clear = vec![key.clone()];

    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![key.clone()];
    while let Some(k) = queue.pop() {
        if !visited.insert(k.clone()) {
            continue;
        }
        to_clear.push(k.clone());
        REVERSE_DEPS.with(|r| {
            if let Some(deps) = r.borrow().get(&k) {
                for d in deps {
                    queue.push(d.clone());
                }
            }
        });
    }
    MODULE_BIND_CACHE.with(|c| {
        if let Some(cache) = c.borrow_mut().as_mut() {
            for k in &to_clear {
                cache.remove(k);
            }
        }
    });
    MODULE_EXPORT_CACHE.with(|c| {
        if let Some(cache) = c.borrow_mut().as_mut() {
            for k in &to_clear {
                cache.remove(k);
            }
        }
    });
    PROGRAM_CACHE.with(|c| {
        if let Some(cache) = c.borrow_mut().as_mut() {
            for k in &to_clear {
                cache.remove(k);
            }
        }
    });
    RESOLVED_PATH_CACHE.with(|c| {
        if let Some(cache) = c.borrow_mut().as_mut() {
            cache.retain(|_, v| !to_clear.contains(v));
        }
    });
}

pub fn invalidate_module_cache() {
    MODULE_BIND_CACHE.with(|c| *c.borrow_mut() = None);
    MODULE_EXPORT_CACHE.with(|c| *c.borrow_mut() = None);
    RESOLVED_PATH_CACHE.with(|c| *c.borrow_mut() = None);
    PROGRAM_CACHE.with(|c| *c.borrow_mut() = None);
    REVERSE_DEPS.with(|r| r.borrow_mut().clear());
    varn_core::clear_interner();
}
