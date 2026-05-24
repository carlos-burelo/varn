use crate::binder::{BindResult, Binder};
use crate::symbol::{Symbol, SymbolKind};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use varn_core::ast::{Decl, ExportDecl, ExportDefaultDecl, Pattern, Stmt, StmtKind};
use varn_core::ModuleId;

thread_local! {
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
}

pub fn invalidate_module_cache() {
    MODULE_BIND_CACHE.with(|c| *c.borrow_mut() = None);
    MODULE_EXPORT_CACHE.with(|c| *c.borrow_mut() = None);
    RESOLVED_PATH_CACHE.with(|c| *c.borrow_mut() = None);
    PROGRAM_CACHE.with(|c| *c.borrow_mut() = None);
    REVERSE_DEPS.with(|r| r.borrow_mut().clear());
}

pub type ExportMap = FxHashMap<String, Symbol>;

pub fn stdlib_path_for(specifier: &str) -> Option<PathBuf> {
    varn_modules::varn_source_for(specifier).map(PathBuf::from)
}

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

    let abs = match stdlib_path_for(specifier) {
        Some(p) => p.to_string_lossy().into_owned(),
        None => return Rc::new(FxHashMap::default()),
    };

    let mut visiting = vec![];
    let result = resolve_module_exports_ref(&abs, &mut visiting);

    export_cache_insert(key, Rc::clone(&result));
    result
}

pub fn resolve_stdlib_module_bind_ref(specifier: &str) -> Option<Rc<BindResult>> {
    let abs = stdlib_path_for(specifier)?;
    resolve_module_bind_ref(&abs.to_string_lossy())
}

pub fn resolve_stdlib_module_bind(specifier: &str) -> Option<BindResult> {
    resolve_stdlib_module_bind_ref(specifier).map(|bind| (*bind).clone())
}

pub fn resolve_module_bind_ref(abs_path: &str) -> Option<Rc<BindResult>> {
    cache_get_or_insert_ref(abs_path)
}

pub fn resolve_module_bind(abs_path: &str) -> Option<BindResult> {
    resolve_module_bind_ref(abs_path).map(|bind| (*bind).clone())
}

pub fn find_module_bind_for_type(type_name: &str, origin_modules: &[String]) -> Option<BindResult> {
    find_module_bind_for_type_ref(type_name, origin_modules).map(|bind| (*bind).clone())
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
    varn_modules::BUILTIN_MODULES.contains(&specifier)
        || varn_modules::STD_MODULES.contains(&specifier)
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

    let joined = base_dir.join(specifier);
    let candidates = if joined.extension().is_some() {
        vec![joined]
    } else {
        vec![joined.with_extension("vn"), joined]
    };

    for candidate in candidates {
        if candidate.exists() {
            let res = varn_modules::canonical_or_original(&candidate);

            RESOLVED_PATH_CACHE.with(|c| {
                let mut guard = c.borrow_mut();
                let cache = guard.get_or_insert_with(FxHashMap::default);
                cache.insert((base_str.clone(), specifier.to_owned()), res.clone());
            });
            return Some(res);
        }
    }

    None
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
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(&source, &canonical_abs);
    let program = Rc::new(varn_parser::parse(tokens, lexeme_buf, &canonical_abs).ok()?);
    PROGRAM_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.entry(canonical_abs.clone()).or_insert_with(|| Rc::clone(&program));
    });
    let mut bind = Binder::bind(&*program);
    for e in lex_errs {
        bind.diagnostics.emit(e);
    }
    let result = Rc::new(bind);
    bind_cache_insert(canonical_abs, Rc::clone(&result));
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

    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(&source, abs_path);
    let program = match varn_parser::parse(tokens, lexeme_buf, abs_path) {
        Ok(p) => {
            for _e in lex_errs {}
            Rc::new(p)
        }
        Err(_) => return FxHashMap::default(),
    };

    PROGRAM_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.insert(abs_path.to_owned(), Rc::clone(&program));
    });

    let bind = if let Some(cached) = bind_cache_get(abs_path) {
        cached
    } else {
        let computed = Rc::new(Binder::bind(&program));
        bind_cache_insert(abs_path.to_owned(), Rc::clone(&computed));
        computed
    };

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
                        s.origin_module = Some(abs_path.to_owned().into());
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
                let src_exports = if is_known_module(source) {
                    resolve_stdlib_module_exports_ref(source)
                } else {
                    let src_abs = resolve_relative(base_dir, source);
                    record_dep(abs_path, &src_abs);
                    resolve_module_exports_ref(&src_abs, visiting)
                };
                let mut ns_sym = Symbol::new(SymbolKind::Namespace, ns.clone(), 0);
                ns_sym.ty = Some(Type::named(ns.to_string()));
                ns_sym.origin_module = Some(abs_path.to_owned().into());
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
        _ => None,
    }
}

fn lookup_global<'a>(bind: &'a BindResult, name: &str) -> Option<&'a Symbol> {
    let scope = bind.scopes.get(bind.global_scope);
    scope.bindings.get(name).map(|&id| bind.arena.get(id))
}

fn resolve_relative(base_dir: &Path, specifier: &str) -> String {
    match varn_core::ImportSpecifier::parse(specifier) {
        varn_core::ImportSpecifier::Package(_) => resolve_package_specifier_path(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
        _ => resolve_specifier_path(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
    }
}

