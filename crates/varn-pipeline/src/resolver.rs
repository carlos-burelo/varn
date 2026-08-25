//! This crate's module resolver.
//!
//! `varn-checker` owns no module cache: every entry point takes an
//! [`ImportResolver`]. Someone still has to hold one, and for a CLI pipeline
//! the honest owner is the pipeline itself — `vn check`, `vn run` and
//! `vn build` each analyse one program on one thread and exit.
//!
//! `thread_local!` here is not the defect this replaced. The defect was a
//! *cache* living in `varn-checker`, invisible in its signatures and invalidated
//! by side effect, which a multi-threaded host silently shared per worker. Here
//! it is one owner, in the crate whose single-threadedness makes it true, and
//! `DiskResolver` is `Rc`-based precisely because it never crosses a thread.
//!
//! The language server does not use this: it owns its own resolver, scoped to
//! the workspace whose files can change.

use std::cell::RefCell;
use varn_checker::module_resolver::DiskResolver;

thread_local! {
    static RESOLVER: RefCell<DiskResolver> = RefCell::new(DiskResolver::new());
}

/// Run `f` against this thread's pipeline resolver.
pub fn with_resolver<R>(f: impl FnOnce(&DiskResolver) -> R) -> R {
    RESOLVER.with(|r| f(&r.borrow()))
}

/// Drop every memoized module and the prelude with it.
pub fn reset() {
    RESOLVER.with(|r| r.borrow().clear());
    varn_core::clear_interner();
}
