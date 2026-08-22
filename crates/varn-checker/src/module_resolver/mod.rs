pub mod cache;
pub mod exports;
pub mod paths;
pub mod stdlib;
pub mod store;

pub use cache::{deserialize_module_interface, serialize_module_interface, ExportMap};
pub use paths::{is_known_module, resolve_package_specifier_path, resolve_specifier_path};
pub use stdlib::{
    resolve_stdlib_module_bind_ref, resolve_stdlib_module_exports,
    resolve_stdlib_module_exports_ref,
};
pub use store::{invalidate_module, invalidate_module_cache};

use crate::binder::{BindResult, Binder};
use crate::module_resolver::cache::try_load_cache;
use crate::module_resolver::store::{
    bind_cache_get, bind_cache_insert, export_cache_get, export_cache_insert, PROGRAM_CACHE,
};
use rustc_hash::FxHashMap;
use std::fs::read_to_string;
use std::path::Path;
use std::rc::Rc;
use varn_core::ast::Program;

pub fn resolve_module_exports(abs_path: &str, visiting: &mut Vec<String>) -> ExportMap {
    resolve_module_exports_ref(abs_path, visiting)
        .as_ref()
        .clone()
}

pub fn resolve_module_exports_ref(abs_path: &str, visiting: &mut Vec<String>) -> Rc<ExportMap> {
    if let Some(cached) = export_cache_get(abs_path) {
        return cached;
    }

    let canonical_abs = varn_modules::canonical_or_original(Path::new(abs_path));

    if canonical_abs != abs_path {
        if let Some(cached) = export_cache_get(&canonical_abs) {
            return cached;
        }
    }

    if visiting.iter().any(|v| v == &canonical_abs) {
        return Rc::new(FxHashMap::default());
    }

    let sentinel = Rc::new(FxHashMap::default());
    export_cache_insert(canonical_abs.clone(), Rc::clone(&sentinel));

    visiting.push(canonical_abs.clone());
    let result = Rc::new(resolve_inner(&canonical_abs, visiting));
    visiting.pop();

    export_cache_insert(canonical_abs, Rc::clone(&result));

    result
}

pub fn resolve_module_bind_ref(abs_path: &str) -> Option<Rc<BindResult>> {
    cache_get_or_insert_ref(abs_path)
}

pub fn resolve_module_bind(abs_path: &str) -> Option<BindResult> {
    resolve_module_bind_ref(abs_path).map(|bind| (*bind).clone())
}

pub fn find_module_bind_for_type_ref(
    type_name: &str,
    origin_modules: &[String],
) -> Option<Rc<BindResult>> {
    for path in origin_modules {
        let bind = resolve_module_bind_ref(path).or_else(|| resolve_stdlib_module_bind_ref(path));
        if let Some(bind) = bind {
            if bind.get_class_entry(type_name).is_some()
                || bind.get_namespace_members_local(type_name).is_some()
                || bind.get_interface_members_local(type_name).is_some()
            {
                return Some(bind);
            }
        }
    }
    None
}

pub(super) fn parse_and_cache(
    source: &str,
    key: &str,
) -> Option<(Rc<Program>, Vec<varn_core::Diagnostic>)> {
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, key);
    let program = Rc::new(varn_parser::parse(tokens, lexeme_buf, key).ok()?);
    PROGRAM_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache
            .entry(key.to_owned())
            .or_insert_with(|| Rc::clone(&program));
    });
    Some((program, lex_errs))
}

pub(super) fn bind_and_cache(
    program: &Program,
    lex_errs: Vec<varn_core::Diagnostic>,
    key: &str,
) -> Rc<BindResult> {
    let mut bind = Binder::bind(program);
    for e in lex_errs {
        bind.diagnostics.emit(e);
    }
    let bind = Rc::new(bind);
    bind_cache_insert(key.to_owned(), Rc::clone(&bind));
    bind
}

pub(super) fn cache_get_or_insert_ref(abs_path: &str) -> Option<Rc<BindResult>> {
    if let Some(cached) = bind_cache_get(abs_path) {
        return Some(cached);
    }

    let canonical_abs = varn_modules::canonical_or_original(Path::new(abs_path));
    let source = read_to_string(&canonical_abs).ok()?;
    if let Some(cached) = try_load_cache(&canonical_abs, &source) {
        let bind_rc = Rc::new(cached.bind);
        bind_cache_insert(canonical_abs.clone(), Rc::clone(&bind_rc));
        let exports_rc = Rc::new(cached.exports);
        export_cache_insert(canonical_abs, exports_rc);
        return Some(bind_rc);
    }
    let (program, lex_errs) = parse_and_cache(&source, &canonical_abs)?;
    let result = bind_and_cache(&program, lex_errs, &canonical_abs);

    let base_dir = Path::new(&canonical_abs).parent().unwrap_or(Path::new("."));
    let mut exports = ExportMap::default();
    let mut visiting = vec![];
    exports::collect_exports(
        &program.body,
        &result,
        &canonical_abs,
        base_dir,
        &mut visiting,
        &mut exports,
    );
    exports::assign_slots(&mut exports);
    cache::save_to_cache(&canonical_abs, &source, &exports, &result);

    Some(result)
}

fn resolve_inner(abs_path: &str, visiting: &mut Vec<String>) -> ExportMap {
    if let Some(cached_bind) = bind_cache_get(abs_path) {
        let program = PROGRAM_CACHE.with(|c| {
            let guard = c.borrow();
            if let Some(cache) = guard.as_ref() {
                return cache.get(abs_path).map(Rc::clone);
            }
            None
        });

        if let Some(program) = program {
            let mut exports = ExportMap::default();
            let base_dir = Path::new(abs_path).parent().unwrap_or(Path::new("."));
            exports::collect_exports(
                &program.body,
                cached_bind.as_ref(),
                abs_path,
                base_dir,
                visiting,
                &mut exports,
            );
            exports::assign_slots(&mut exports);
            return exports;
        }
    }

    let source = match read_to_string(abs_path) {
        Ok(s) => s,
        Err(_) => return FxHashMap::default(),
    };

    let Some((program, _lex_errs)) = parse_and_cache(&source, abs_path) else {
        return FxHashMap::default();
    };

    let bind =
        bind_cache_get(abs_path).unwrap_or_else(|| bind_and_cache(&program, Vec::new(), abs_path));

    let mut exports = ExportMap::default();
    let base_dir = Path::new(abs_path).parent().unwrap_or(Path::new("."));
    exports::collect_exports(
        &program.body,
        bind.as_ref(),
        abs_path,
        base_dir,
        visiting,
        &mut exports,
    );
    exports::assign_slots(&mut exports);
    exports
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_bind_result() -> BindResult {
        let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan("", "<test>");
        let program = varn_parser::parse(tokens, lexeme_buf, "<test>").unwrap();
        Binder::bind(&program)
    }

    #[test]
    fn interface_blob_roundtrip() {
        let exports: ExportMap = FxHashMap::default();
        let bind = empty_bind_result();
        let bytes = serialize_module_interface(&exports, &bind).unwrap();
        let (e2, _b2) = deserialize_module_interface(&bytes).unwrap();
        assert_eq!(e2.len(), 0);
    }
}
