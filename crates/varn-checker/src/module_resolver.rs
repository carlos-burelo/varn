use crate::binder::{BindResult, Binder};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::fs::read_to_string;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, Pattern, Program, Stmt, StmtKind};
use varn_core::ModuleId;

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedModule {
    exports: ExportMap,
    bind: BindResult,
}

/// Serialize a module's checker interface for embedding in a std bundle.
/// Format = the .vnm CachedModule payload (BUILD_FINGERPRINT governs it;
/// the bundle records that version and vn rejects mismatches at load).
pub fn serialize_module_interface(
    exports: &ExportMap,
    bind: &BindResult,
) -> Result<Vec<u8>, String> {
    let cached = CachedModule {
        exports: exports.clone(),
        bind: bind.clone(),
    };
    postcard::to_allocvec(&cached).map_err(|e| e.to_string())
}

pub fn deserialize_module_interface(bytes: &[u8]) -> Result<(ExportMap, BindResult), String> {
    let mut cached: CachedModule = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
    assign_slots(&mut cached.exports);
    Ok((cached.exports, cached.bind))
}

fn compute_source_hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

fn get_cache_dir() -> std::path::PathBuf {
    PROJECT_ROOT.with(|r| {
        let mut guard = r.borrow_mut();
        if guard.is_none() {
            let current_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let root = varn_modules::artifact::find_project_root(&current_dir);
            *guard = Some(root);
        }
        varn_modules::artifact::get_types_cache_dir(guard.as_ref().unwrap())
    })
}

fn try_load_cache(virtual_id: &str, source: &str) -> Option<CachedModule> {
    if virtual_id == "std:types" {
        return None;
    }
    let hash = compute_source_hash(source);
    let name = if virtual_id.contains(':') {
        virtual_id.replace(':', "_")
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        virtual_id.hash(&mut hasher);
        format!("file_{:x}", hasher.finish())
    };

    let cache_dir = get_cache_dir();
    let cache_file = cache_dir.join(format!("{}.{:x}.vnm", name, hash));
    if !cache_file.exists() {
        return None;
    }
    let bytes = std::fs::read(&cache_file).ok()?;
    let payload = match varn_modules::artifact::read_envelope(
        varn_modules::artifact::MAGIC_VNM,
        varn_modules::artifact::BUILD_FINGERPRINT,
        &bytes,
    ) {
        Ok(p) => p,
        Err(_) => return None,
    };
    match postcard::from_bytes::<CachedModule>(payload) {
        Ok(mut val) => {
            assign_slots(&mut val.exports);
            Some(val)
        }
        Err(_) => None,
    }
}

fn save_to_cache(virtual_id: &str, source: &str, exports: &ExportMap, bind: &BindResult) {
    if virtual_id == "std:types" {
        return;
    }
    let hash = compute_source_hash(source);
    let cache_dir = get_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let name = if virtual_id.contains(':') {
        virtual_id.replace(':', "_")
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        virtual_id.hash(&mut hasher);
        format!("file_{:x}", hasher.finish())
    };

    let cache_file = cache_dir.join(format!("{}.{:x}.vnm", name, hash));
    let cached = CachedModule {
        exports: exports.clone(),
        bind: bind.clone(),
    };
    match postcard::to_allocvec(&cached) {
        Ok(payload) => {
            let bytes = varn_modules::artifact::write_envelope(
                varn_modules::artifact::MAGIC_VNM,
                varn_modules::artifact::BUILD_FINGERPRINT,
                &payload,
            );
            let _ = std::fs::write(&cache_file, bytes);
        }
        Err(_) => {}
    }
}

thread_local! {
    static PROJECT_ROOT: RefCell<Option<std::path::PathBuf>> = RefCell::new(None);
    static MODULE_BIND_CACHE: RefCell<Option<FxHashMap<String, Rc<BindResult>>>> = RefCell::new(None);
    static MODULE_EXPORT_CACHE: RefCell<Option<FxHashMap<String, Rc<ExportMap>>>> = RefCell::new(None);
    static RESOLVED_PATH_CACHE: RefCell<Option<FxHashMap<(String, String), String>>> = RefCell::new(None);
    static PROGRAM_CACHE: RefCell<Option<FxHashMap<String, Rc<varn_core::ast::Program>>>> = RefCell::new(None);
    static REVERSE_DEPS: RefCell<FxHashMap<String, Vec<String>>> = RefCell::new(FxHashMap::default());
}

fn export_cache_get(key: &str) -> Option<Rc<ExportMap>> {
    MODULE_EXPORT_CACHE.with(|c| {
        let guard = c.borrow();
        let cache = guard.as_ref()?;
        cache.get(key).map(Rc::clone)
    })
}

fn export_cache_insert(key: String, exports: Rc<ExportMap>) {
    MODULE_EXPORT_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.insert(key, exports);
    });
}

fn bind_cache_get(key: &str) -> Option<Rc<BindResult>> {
    MODULE_BIND_CACHE.with(|c| {
        let guard = c.borrow();
        let cache = guard.as_ref()?;
        cache.get(key).map(Rc::clone)
    })
}

fn bind_cache_insert(key: String, bind: Rc<BindResult>) {
    MODULE_BIND_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.entry(key).or_insert(bind);
    });
}

/// Bundle-first resolution: if the active std bundle carries a precomputed
/// interface blob for this module, decode it and populate both caches.
/// Corrupt blob = hard error (no fallback), matching the bundle's compat gates.
fn resolve_from_interface_blob(
    specifier: &str,
    key: &String,
) -> Option<(Rc<ExportMap>, Rc<BindResult>)> {
    let provider = varn_modules::provider::get()?;
    let blob = provider.interface_blob(specifier)?;
    match deserialize_module_interface(blob) {
        Ok((exports, bind)) => {
            let exports = Rc::new(exports);
            let bind = Rc::new(bind);
            export_cache_insert(key.clone(), Rc::clone(&exports));
            bind_cache_insert(key.clone(), Rc::clone(&bind));
            Some((exports, bind))
        }
        Err(e) => panic!("corrupt interface blob for {specifier}: {e}"),
    }
}

fn record_dep(importer: &str, imported: &str) {
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

pub type ExportMap = FxHashMap<String, Symbol>;

pub fn resolve_stdlib_module_exports(specifier: &str) -> ExportMap {
    resolve_stdlib_module_exports_ref(specifier)
        .as_ref()
        .clone()
}

pub fn resolve_stdlib_module_exports_ref(specifier: &str) -> Rc<ExportMap> {
    let id = ModuleId::stdlib(specifier);
    let key = id.as_str();

    if let Some(cached) = export_cache_get(&key) {
        return cached;
    }

    if specifier == "std:types" {
        if let Some(provider) = varn_modules::provider::get() {
            if let Some(source) = provider.embedded_source(specifier) {
                let mut visiting = vec![];
                let result = resolve_from_embedded_source(specifier, source, &mut visiting);
                export_cache_insert(key.to_string(), Rc::clone(&result));
                return result;
            }
        }
    }

    // The interface blob and the module source are TWO type surfaces for the
    // same stdlib, and they do not agree: measured 2026-08-15, the blob loses
    // `Option<T>` (`Some("x")` infers `str?`), and resolving from source
    // instead loses the `Partial<T>` / `Readonly<T>` utility types. Neither is
    // complete, so the order below is not a preference — it is the one that
    // happens to satisfy the current test corpus. See the note in
    // docs/STDLIB_ARCHITECTURE.md before changing it.
    if let Some((exports, _bind)) = resolve_from_interface_blob(specifier, &key) {
        return exports;
    }

    if let Some(provider) = varn_modules::provider::get() {
        if let Some(source) = provider.embedded_source(specifier) {
            let mut visiting = vec![];
            let result = resolve_from_embedded_source(specifier, source, &mut visiting);
            export_cache_insert(key.to_string(), Rc::clone(&result));
            return result;
        }
        if let Some(path) = provider.source_path(specifier) {
            let abs = path.to_string_lossy().into_owned();
            let mut visiting = vec![];
            let result = resolve_module_exports_ref(&abs, &mut visiting);
            export_cache_insert(key.to_string(), Rc::clone(&result));
            return result;
        }
    }

    Rc::new(FxHashMap::default())
}

pub fn resolve_stdlib_module_bind_ref(specifier: &str) -> Option<Rc<BindResult>> {
    let id = ModuleId::stdlib(specifier);
    let key = id.as_str();

    if let Some(cached) = bind_cache_get(&key) {
        return Some(cached);
    }

    if let Some((_exports, bind)) = resolve_from_interface_blob(specifier, &key) {
        return Some(bind);
    }

    let provider = varn_modules::provider::get()?;

    if let Some(source) = provider.embedded_source(specifier) {
        return bind_from_embedded_source(specifier, source);
    }
    let path = provider.source_path(specifier)?;
    cache_get_or_insert_ref(&path.to_string_lossy())
}

/// Lex + parse one module source and memoize the `Program` under `key`.
///
/// `None` means the parse failed; what an unparseable dependency means is the
/// caller's decision (an empty export map, or giving up on the bind). The lexer
/// diagnostics come back rather than being folded in here: only the callers that
/// build a `BindResult` have somewhere to put them.
fn parse_and_cache(source: &str, key: &str) -> Option<(Rc<Program>, Vec<varn_core::Diagnostic>)> {
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

/// Bind `program`, fold `lex_errs` into its diagnostics, and memoize the result
/// under `key`.
///
/// The insert happens before the caller collects exports, so a circular import
/// that comes back around finds this bind instead of parsing the module again.
fn bind_and_cache(
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

fn resolve_from_embedded_source(
    virtual_id: &str,
    source: &str,
    visiting: &mut Vec<String>,
) -> Rc<ExportMap> {
    if visiting.iter().any(|v| v == virtual_id) {
        return Rc::new(FxHashMap::default());
    }
    visiting.push(virtual_id.to_owned());

    if let Some(cached) = try_load_cache(virtual_id, source) {
        let bind_rc = Rc::new(cached.bind);
        let exports_rc = Rc::new(cached.exports);

        bind_cache_insert(virtual_id.to_owned(), Rc::clone(&bind_rc));
        visiting.pop();
        return exports_rc;
    }

    // Lex diagnostics are dropped here: this path produces an export map, not
    // diagnostics -- the module gets checked on its own account elsewhere.
    let Some((program, _lex_errs)) = parse_and_cache(source, virtual_id) else {
        visiting.pop();
        return Rc::new(FxHashMap::default());
    };

    let bind = bind_and_cache(&program, Vec::new(), virtual_id);

    let base_dir = Path::new(".");
    let mut exports = ExportMap::default();
    collect_exports(
        &program.body,
        bind.as_ref(),
        virtual_id,
        base_dir,
        visiting,
        &mut exports,
    );
    assign_slots(&mut exports);

    save_to_cache(virtual_id, source, &exports, bind.as_ref());

    visiting.pop();
    Rc::new(exports)
}

fn bind_from_embedded_source(virtual_id: &str, source: &str) -> Option<Rc<BindResult>> {
    if let Some(cached) = bind_cache_get(virtual_id) {
        return Some(cached);
    }
    if let Some(cached) = try_load_cache(virtual_id, source) {
        let bind_rc = Rc::new(cached.bind);
        bind_cache_insert(virtual_id.to_owned(), Rc::clone(&bind_rc));
        let exports_rc = Rc::new(cached.exports);
        let id = ModuleId::stdlib(virtual_id);
        export_cache_insert(id.as_str().to_owned(), exports_rc);
        return Some(bind_rc);
    }
    let (program, lex_errs) = parse_and_cache(source, virtual_id)?;
    Some(bind_and_cache(&program, lex_errs, virtual_id))
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

pub fn is_known_module(specifier: &str) -> bool {
    varn_modules::resolver::is_known_module(specifier)
}

pub fn resolve_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    let base_str = base_dir.to_string_lossy().into_owned();
    let cached = RESOLVED_PATH_CACHE.with(|c| {
        let guard = c.borrow();
        if let Some(cache) = guard.as_ref() {
            return cache
                .get(&(base_str.clone(), specifier.to_owned()))
                .cloned();
        }
        None
    });
    if let Some(res) = cached {
        return Some(res);
    }

    let res = varn_modules::resolver::resolve_specifier_path(base_dir, specifier)?;

    RESOLVED_PATH_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.insert((base_str.clone(), specifier.to_owned()), res.clone());
    });
    Some(res)
}

pub fn resolve_package_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    varn_modules::resolve_pkg_specifier(base_dir, specifier)
}

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

fn cache_get_or_insert_ref(abs_path: &str) -> Option<Rc<BindResult>> {
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
    collect_exports(
        &program.body,
        &result,
        &canonical_abs,
        base_dir,
        &mut visiting,
        &mut exports,
    );
    assign_slots(&mut exports);
    save_to_cache(&canonical_abs, &source, &exports, &result);

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
            collect_exports(
                &program.body,
                cached_bind.as_ref(),
                abs_path,
                base_dir,
                visiting,
                &mut exports,
            );
            assign_slots(&mut exports);
            return exports;
        }
    }

    let source = match read_to_string(abs_path) {
        Ok(s) => s,
        Err(_) => return FxHashMap::default(),
    };

    // Same as above: an export map carries no diagnostics.
    let Some((program, _lex_errs)) = parse_and_cache(&source, abs_path) else {
        return FxHashMap::default();
    };

    let bind =
        bind_cache_get(abs_path).unwrap_or_else(|| bind_and_cache(&program, Vec::new(), abs_path));

    let mut exports = ExportMap::default();
    let base_dir = Path::new(abs_path).parent().unwrap_or(Path::new("."));
    collect_exports(
        &program.body,
        bind.as_ref(),
        abs_path,
        base_dir,
        visiting,
        &mut exports,
    );
    assign_slots(&mut exports);
    exports
}

fn assign_slots(exports: &mut ExportMap) {
    let mut keys: Vec<String> = exports.keys().cloned().collect();
    keys.sort();
    for (idx, key) in keys.iter().enumerate() {
        if let Some(sym) = exports.get_mut(key) {
            sym.slot_idx = Some(idx);
        }
    }
}

fn collect_exports(
    stmts: &[Stmt],
    bind: &BindResult,
    abs_path: &str,
    base_dir: &Path,
    visiting: &mut Vec<String>,
    out: &mut ExportMap,
) {
    for stmt in stmts {
        let StmtKind::Decl(decl) = &stmt.kind else {
            continue;
        };
        let Decl::Export(e) = decl.as_ref() else {
            continue;
        };

        match e {
            ExportDecl::Decl { declaration, .. } => {
                if let Some(name) = decl_primary_name(declaration) {
                    if let Some(sym) = lookup_global(bind, &name) {
                        let mut s = sym.clone();
                        s.origin_module = Some(abs_path.to_owned().into());
                        out.insert(name.to_string(), s);
                    }
                }
                if let Decl::SumType(st) = declaration.as_ref() {
                    for variant in &st.variants {
                        if let Some(sym) = lookup_global(bind, &variant.name) {
                            let mut s = sym.clone();
                            s.origin_module = Some(abs_path.to_owned().into());
                            out.insert(variant.name.to_string(), s);
                        }
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: None,
                ..
            } => {
                for spec in specifiers {
                    if let Some(sym) = lookup_global(bind, &spec.local) {
                        let mut s = sym.clone();
                        s.name = spec.exported.clone();
                        // A re-exported import keeps its DECLARING module as
                        // origin (member lookup resolves the class there);
                        // only locally-declared symbols get stamped with this
                        // module. Mirrors the `export { x } from "src"` arm.
                        s.origin_module = s
                            .origin_module
                            .take()
                            .or_else(|| Some(abs_path.to_owned().into()));
                        out.insert(spec.exported.to_string(), s);
                    }
                }
            }
            ExportDecl::Named {
                specifiers,
                source: Some(src),
                ..
            } => {
                let src_exports = if is_known_module(src) {
                    resolve_stdlib_module_exports_ref(src)
                } else {
                    let src_abs = resolve_relative(base_dir, src);
                    record_dep(abs_path, &src_abs);
                    resolve_module_exports_ref(&src_abs, visiting)
                };
                for spec in specifiers {
                    if let Some(sym) = src_exports.get(&spec.local.to_string()) {
                        let mut s = sym.clone();
                        s.name = spec.exported.clone();
                        s.re_export_path.push(abs_path.to_owned().into());
                        out.insert(spec.exported.to_string(), s);
                    }
                }
            }
            ExportDecl::All {
                source,
                alias: None,
                ..
            } => {
                let src_exports = if is_known_module(source) {
                    resolve_stdlib_module_exports_ref(source)
                } else {
                    let src_abs = resolve_relative(base_dir, source);
                    record_dep(abs_path, &src_abs);
                    resolve_module_exports_ref(&src_abs, visiting)
                };
                for (name, sym) in src_exports.iter() {
                    out.entry(name.clone()).or_insert_with(|| {
                        let mut s = sym.clone();
                        s.re_export_path.push(abs_path.to_owned().into());
                        s
                    });
                }
            }
            ExportDecl::All {
                source,
                alias: Some(ns),
                ..
            } => {
                let src_abs = if is_known_module(source) {
                    source.to_string()
                } else {
                    let src_abs = resolve_relative(base_dir, source);
                    record_dep(abs_path, &src_abs);
                    src_abs
                };
                let src_exports = if is_known_module(source) {
                    resolve_stdlib_module_exports_ref(source)
                } else {
                    resolve_module_exports_ref(&src_abs, visiting)
                };
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, ns.clone(), 0);
                ns_sym.ty = Some(Type::named_with_origin("*", Some(src_abs.clone())));
                ns_sym.origin_module = Some(src_abs.into());
                for (sub_name, sub_sym) in src_exports.iter() {
                    let mut s = sub_sym.clone();
                    s.re_export_path.push(abs_path.to_owned().into());
                    out.insert(format!("{ns}.{sub_name}"), s);
                }
                out.insert(ns.to_string(), ns_sym);
            }
            ExportDecl::Default { declaration, .. } => match declaration.as_ref() {
                ExportDefaultDecl::Function(f) => {
                    if let Some(sym) = lookup_global(bind, &f.id) {
                        let mut s = sym.clone();
                        s.name = "default".into();
                        out.insert("default".into(), s);
                    }
                }
                ExportDefaultDecl::Class(c) => {
                    if let Some(id) = &c.id {
                        if let Some(sym) = lookup_global(bind, id) {
                            let mut s = sym.clone();
                            s.name = "default".into();
                            out.insert("default".into(), s);
                        }
                    }
                }
                ExportDefaultDecl::Expr(_expr) => {
                    let mut s = Symbol::new(SymbolKind::Let, "default".into(), 0);
                    s.origin_module = Some(abs_path.to_owned().into());
                    out.insert("default".into(), s);
                }
            },
        }
    }
}

fn decl_primary_name(decl: &Decl) -> Option<Rc<str>> {
    match decl {
        Decl::Variable(v) => v.declarators.first().and_then(|d| match &d.id {
            Pattern::Identifier { name, .. } => Some(name.clone()),
            _ => None,
        }),
        Decl::Function(f) => Some(f.id.clone()),
        Decl::Class(c) => c.id.clone(),
        Decl::Enum(e) => Some(e.id.clone()),
        Decl::Interface(i) => Some(i.id.clone()),
        Decl::TypeAlias(t) => Some(t.id.clone()),
        Decl::Namespace(n) => Some(n.id.clone()),
        Decl::Struct(s) => Some(s.id.clone()),
        Decl::SumType(s) => Some(s.id.clone()),
        _ => None,
    }
}

fn lookup_global<'a>(bind: &'a BindResult, name: &str) -> Option<&'a Symbol> {
    let scope = bind.scopes.get(bind.global_scope);
    scope.bindings.get(name).map(|&id| bind.arena.get(id))
}

fn resolve_relative(base_dir: &Path, specifier: &str) -> String {
    let raw = match varn_core::ImportSpecifier::parse(specifier) {
        varn_core::ImportSpecifier::Package(_) => {
            resolve_package_specifier_path(base_dir, specifier)
                .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned())
        }
        _ => resolve_specifier_path(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
    };
    varn_modules::canonical_or_original(Path::new(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BindResult` has no `Default` impl (its fields include arenas and a
    /// source file `Rc<str>` with no zero-value meaning), so the cheapest
    /// valid instance is binding an empty parsed program, matching how the
    /// rest of this module constructs a `BindResult` from source.
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
