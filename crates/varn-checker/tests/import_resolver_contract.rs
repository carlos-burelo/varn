#![allow(unused_crate_dependencies)]

//! Contract tests for [`ImportResolver`]'s derived behaviour.
//!
//! `find_bind_for_type` searches a *list* of candidate modules, but every
//! caller today passes an `Option`, i.e. zero or one. That makes its
//! multi-candidate behaviour unreachable from `tests/main.vn` — and it is
//! exactly where a regression hid: an early `?` turned "skip this candidate"
//! into "abandon the search", indistinguishable from correct behaviour at a
//! list length of one.
//!
//! A stub resolver pins the contract at lengths the real callers do not yet
//! reach, so the next caller that passes two origins does not pay for it.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use varn_checker::binder::{BindResult, Binder};
use varn_checker::module_resolver::{ExportMap, ImportResolver};

/// Binds `source`, yielding a real `BindResult` to hand back from the stub.
///
/// A throwaway `DiskResolver` suffices: these fixtures import nothing, so the
/// binder never asks it anything.
fn bind(source: &str) -> Rc<BindResult> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, "<stub>");
    let program = varn_parser::parse(tokens, lexeme_buf, "<stub>").expect("stub must parse");
    let resolver = varn_checker::module_resolver::DiskResolver::new();
    Rc::new(Binder::bind(&program, &resolver))
}

/// Answers from a fixed table; anything absent fails to resolve. Records the
/// order it was asked, so a test can prove the search did not stop early.
struct StubResolver {
    modules: Vec<(String, Rc<BindResult>)>,
    asked: RefCell<Vec<String>>,
}

impl StubResolver {
    fn new(modules: Vec<(&str, Rc<BindResult>)>) -> Self {
        Self {
            modules: modules
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl ImportResolver for StubResolver {
    fn module_bind(&self, abs_path: &str) -> Option<Rc<BindResult>> {
        self.asked.borrow_mut().push(abs_path.to_owned());
        self.modules
            .iter()
            .find(|(k, _)| k == abs_path)
            .map(|(_, v)| Rc::clone(v))
    }

    fn module_exports(&self, _abs_path: &str, _visiting: &mut Vec<String>) -> Rc<ExportMap> {
        Rc::new(ExportMap::default())
    }

    fn stdlib_bind(&self, _specifier: &str) -> Option<Rc<BindResult>> {
        None
    }

    fn stdlib_exports(&self, _specifier: &str) -> Rc<ExportMap> {
        Rc::new(ExportMap::default())
    }

    fn resolve_specifier(&self, _base_dir: &Path, _specifier: &str) -> Option<String> {
        None
    }

    fn record_dep(&self, _importer: &str, _imported: &str) {}

    fn core_exports(
        &self,
    ) -> Rc<rustc_hash::FxHashMap<Rc<str>, varn_checker::symbol::Symbol>> {
        Rc::new(rustc_hash::FxHashMap::default())
    }

    fn core_members(&self) -> Rc<varn_checker::core::loader::CoreMembers> {
        Rc::new(varn_checker::core::loader::CoreMembers::default())
    }
}

/// The regression. An unresolvable candidate must be skipped, not treated as
/// the end of the search.
#[test]
fn an_unresolvable_candidate_does_not_abandon_the_search() {
    let holder = bind("export class Widget { value: int = 1 }");
    let resolver = StubResolver::new(vec![("has_it", holder)]);

    let origins = vec!["missing".to_owned(), "has_it".to_owned()];
    let found = resolver.find_bind_for_type("Widget", &origins);

    assert!(
        found.is_some(),
        "`Widget` lives in the second candidate; the first failing to resolve \
         must not end the search"
    );
    assert_eq!(
        *resolver.asked.borrow(),
        vec!["missing".to_owned(), "has_it".to_owned()],
        "both candidates must be consulted, in order"
    );
}

/// A candidate that resolves but does not declare the type is also skipped.
#[test]
fn a_candidate_without_the_type_does_not_abandon_the_search() {
    let unrelated = bind("export class Other { n: int = 0 }");
    let holder = bind("export class Widget { value: int = 1 }");
    let resolver = StubResolver::new(vec![("unrelated", unrelated), ("has_it", holder)]);

    let origins = vec!["unrelated".to_owned(), "has_it".to_owned()];
    assert!(
        resolver.find_bind_for_type("Widget", &origins).is_some(),
        "a resolvable module that lacks the type must not end the search"
    );
}

#[test]
fn no_candidate_declaring_the_type_yields_none() {
    let unrelated = bind("export class Other { n: int = 0 }");
    let resolver = StubResolver::new(vec![("unrelated", unrelated)]);

    assert!(resolver
        .find_bind_for_type("Widget", &["unrelated".to_owned()])
        .is_none());
    assert!(resolver.find_bind_for_type("Widget", &[]).is_none());
}
