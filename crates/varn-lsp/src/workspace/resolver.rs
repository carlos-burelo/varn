//! The language server's module resolver.
//!
//! `varn-checker` owns no module cache, so the server owns one here.
//!
//! **This is still per-thread, and that is still the open defect.**
//! `DiskResolver` memoizes with `Rc`/`RefCell`, so it cannot be shared across
//! the blocking pool that analysis currently runs on; every worker therefore
//! builds and invalidates its own copy, and cross-module resolution can differ
//! depending on which worker answered.
//!
//! What phase 2 changed is that this is now the *only* place it can happen, and
//! it is visible: the checker no longer hides a cache behind its signatures, so
//! confining analysis to one thread (L5, the analysis actor) collapses this to a
//! single coherent graph without touching the checker again.
//!
//! See `docs/LSP_ARCHITECTURE.md`, L4/L5.

use std::cell::RefCell;
use varn_checker::module_resolver::DiskResolver;

thread_local! {
    static RESOLVER: RefCell<DiskResolver> = RefCell::new(DiskResolver::new());
}

/// Run `f` against this thread's resolver.
pub fn with_resolver<R>(f: impl FnOnce(&DiskResolver) -> R) -> R {
    RESOLVER.with(|r| f(&r.borrow()))
}

/// Drop every memoized module and the prelude with it.
pub fn reset() {
    RESOLVER.with(|r| r.borrow().clear());
    varn_core::clear_interner();
}

/// Evict `id` and everything that transitively imports it.
pub fn invalidate(id: &varn_core::ModuleId) {
    RESOLVER.with(|r| r.borrow().invalidate(id));
}
