
















use std::cell::RefCell;
use varn_checker::module_resolver::DiskResolver;

thread_local! {
    static RESOLVER: RefCell<DiskResolver> = RefCell::new(DiskResolver::new());
}


pub fn with_resolver<R>(f: impl FnOnce(&DiskResolver) -> R) -> R {
    RESOLVER.with(|r| f(&r.borrow()))
}


pub fn reset() {
    RESOLVER.with(|r| r.borrow().clear());
    varn_core::clear_interner();
}


pub fn invalidate(id: &varn_core::ModuleId) {
    RESOLVER.with(|r| r.borrow().invalidate(id));
}
