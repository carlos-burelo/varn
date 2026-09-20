use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use varn_checker::module_resolver::ImportResolver;

use rustc_hash::FxHashMap;
use varn_core::ModuleId;
use varn_modules::loader::ModuleLoader as CanonicalLoader;
use varn_types::FunctionProto;
use varn_vm::loader::{ModuleError, ModuleLoader};

// ---------------------------------------------------------------------------
// Compiled-proto caches shared by FileLoader and StdlibLoader.
//
// Per-thread materialized protos plus process-wide postcard bytes:
// `Rc<FunctionProto>` is not `Send`, and isolate workers each run a fresh VM
// on a fresh thread, so cross-thread reuse shares the serialized form and
// deserializes once per thread instead of recompiling. Every VM instance
// still evaluates the module body itself; only lexing/parsing/checking/
// compiling is shared.
//
// Entries carry a source fingerprint: local files invalidate when their
// content changes; stdlib specs use `STD_FINGERPRINT` because the active std
// is process-fixed (see `std_root::resolve`).
// ---------------------------------------------------------------------------

thread_local! {
    static PROTO_CACHE: RefCell<FxHashMap<String, (u64, Rc<FunctionProto>)>> =
        RefCell::new(FxHashMap::default());
}

type CompiledBytesMap = FxHashMap<String, (u64, Arc<[u8]>)>;
static COMPILED_BYTES: Mutex<Option<CompiledBytesMap>> = Mutex::new(None);

const STD_FINGERPRINT: u64 = 0;

fn source_fingerprint(source: &str) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    source.hash(&mut h);
    h.finish()
}

fn cached_proto(key: &str, fingerprint: u64) -> Option<Rc<FunctionProto>> {
    let thread_hit = PROTO_CACHE.with(|c| c.borrow().get(key).cloned());
    if let Some((fp, proto)) = thread_hit {
        if fp == fingerprint {
            return Some(proto);
        }
    }
    let bytes = {
        let guard = COMPILED_BYTES.lock().ok()?;
        let map = guard.as_ref()?;
        let (fp, bytes) = map.get(key)?;
        if *fp != fingerprint {
            return None;
        }
        bytes.clone()
    };
    let proto = Rc::new(postcard::from_bytes::<FunctionProto>(&bytes).ok()?);
    PROTO_CACHE.with(|c| {
        c.borrow_mut()
            .insert(key.to_owned(), (fingerprint, proto.clone()));
    });
    Some(proto)
}

fn store_proto(key: &str, fingerprint: u64, proto: &Rc<FunctionProto>) {
    if let Ok(bytes) = postcard::to_allocvec(proto.as_ref()) {
        if let Ok(mut guard) = COMPILED_BYTES.lock() {
            guard.get_or_insert_with(FxHashMap::default).insert(
                key.to_owned(),
                (fingerprint, Arc::from(bytes.into_boxed_slice())),
            );
        }
    }
    PROTO_CACHE.with(|c| {
        c.borrow_mut()
            .insert(key.to_owned(), (fingerprint, proto.clone()));
    });
}

pub struct FileLoader;

impl ModuleLoader for FileLoader {
    fn resolve(&self, spec: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        // One resolution: the same `ModuleResolver` every other consumer uses.
        // This loader only ROUTES by scheme (local files), it does not resolve.
        varn_modules::resolver::ModuleResolver::new()
            .resolve(spec, from)
            .map_err(ModuleError::new)
            .and_then(|id| match id {
                ModuleId::Local(_) => Ok(id),
                _ => Err(ModuleError::new(format!(
                    "FileLoader cannot resolve non-local specifier: {spec}"
                ))),
            })
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        let path = match id {
            ModuleId::Local(p) => p.as_ref(),
            _ => return Ok(None),
        };
        // Source through the canonical registry (single door), not `fs` here.
        let source = CanonicalLoader::source(&varn_modules::loader::default_registry(), id)
            .map_err(|e| ModuleError::new(e.to_string()))?
            .text;
        let fingerprint = varn_modules::artifact::source_fingerprint(source.as_ref());
        let key = varn_modules::artifact::module_key(id, fingerprint);
        if let Some(hit) = cached_proto(&key, fingerprint) {
            return Ok(Some(hit));
        }
        let proto = compile_source(source.as_ref(), path)
            .map(Rc::new)
            .map_err(|e| ModuleError::new(format!("compile error in '{path}': {e}")))?;
        store_proto(&key, fingerprint, &proto);
        Ok(Some(proto))
    }

    fn native(&self, _id: &ModuleId) -> Option<varn_types::Value> {
        None
    }
}

pub struct StdlibLoader;

impl ModuleLoader for StdlibLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        // One resolution (shared); this loader only routes by scheme.
        varn_modules::resolver::ModuleResolver::new()
            .resolve(specifier, from)
            .map_err(ModuleError::new)
            .and_then(|id| match id {
                ModuleId::Std(_) | ModuleId::Core(_) | ModuleId::Runtime(_) => Ok(id),
                _ => Err(ModuleError::new(format!(
                    "StdlibLoader cannot resolve non-stdlib specifier: {specifier}"
                ))),
            })
    }

    fn native(&self, _id: &ModuleId) -> Option<varn_types::Value> {
        None
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        let spec = match id {
            ModuleId::Std(s) | ModuleId::Core(s) => s.as_ref(),
            ModuleId::Runtime(_) => return Ok(None),
            _ => return Ok(None),
        };

        // Same key scheme as every other per-module cache (ADR-0011).
        let key = varn_modules::artifact::module_key(id, STD_FINGERPRINT);
        if let Some(hit) = cached_proto(&key, STD_FINGERPRINT) {
            return Ok(Some(hit));
        }
        let proto = Rc::new(load_uncached(id, spec)?);
        store_proto(&key, STD_FINGERPRINT, &proto);
        Ok(Some(proto))
    }
}

fn load_uncached(id: &ModuleId, spec: &str) -> Result<FunctionProto, ModuleError> {
    // Same single door as the checker: the registry gives the text (and the
    // precompiled bytecode when the bundle ships it).
    let source = CanonicalLoader::source(&varn_modules::loader::default_registry(), id)
        .map_err(|e| ModuleError::new(e.to_string()))?;

    if let Some(blob) = source.bytecode.as_ref() {
        return postcard::from_bytes(blob)
            .map_err(|e| ModuleError::new(format!("corrupt std bundle bytecode for {spec}: {e}")));
    }

    compile_source(source.text.as_ref(), spec)
        .map_err(|e| ModuleError::new(format!("stdlib compile error in {spec}: {e}")))
}

/// Compile one module source to a FunctionProto. Used by StdlibLoader, which
/// also loads user modules.
pub fn compile_source(source: &str, path: &str) -> Result<FunctionProto, String> {
    compile_source_inner(source, path, false)
}

/// Same, but rejects a module whose types do not check.
///
/// Only the bundle build validates: it compiles every stdlib module once, in
/// manifest order, so its diagnostics are complete. The on-demand loader
/// resolves modules in import order and would re-litigate types that were
/// already validated — and a load-time `Err` on a module the program needs
/// deadlocks rather than reporting.
pub fn compile_source_checked(source: &str, path: &str) -> Result<FunctionProto, String> {
    compile_source_inner(source, path, true)
}

fn compile_source_inner(
    source: &str,
    path: &str,
    reject_type_errors: bool,
) -> Result<FunctionProto, String> {
    let (program, arena, interner) = crate::quiet_parse::parse_module(source, path, "")?;
    let check = crate::resolver::with_resolver(|r| {
        varn_checker::Checker::check(&program, &arena, interner, r)
    });
    if reject_type_errors && check.diagnostics.has_errors() {
        // The stdlib goes through the same checker as user code. Silently
        // dropping these diagnostics let `std/*.vn` carry types the backend
        // then trusted — e.g. an `int`-declared function returning a whole
        // float, which clif then reinterprets as an int.
        let mut msg = String::new();
        for d in check.diagnostics.errors() {
            msg.push_str(&format!(
                "\n  {path}:{}:{}: {}",
                d.range.start.line, d.range.start.column, d.message
            ));
        }
        return Err(format!("type errors in stdlib module:{msg}"));
    }
    let exports =
        if path.starts_with("std:") || path.starts_with("core:") || path.starts_with("runtime:") {
            crate::resolver::with_resolver(|r| r.stdlib_exports(path))
        } else {
            crate::resolver::with_resolver(|r| r.module_exports(path, &mut vec![]))
        };
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();
    let tir = varn_checker::emit::emit_module(
        &program,
        &arena,
        &check.bind,
        &check.expr_table,
        &check.call_mappings,
        &check.extension_calls,
        &check.extension_members,
        &check.extension_set_members,
    );
    varn_compiler::from_tir::compile_module(&tir, export_names).map_err(|e| format!("{e:?}"))
}

/// Spec §2: std modules may only import `runtime:*`, `std:*` or `core:intrinsics`.
///
/// Parser-based (not string-scanning): lex + parse the module, then reuse the
/// same import collector the pipeline uses for cache invalidation. Avoids the
/// false positives/negatives of matching import syntax inside string literals.
fn validate_imports(id: &str, source: &str) -> Result<(), String> {
    let (program, arena, interner) = crate::quiet_parse::parse_only(source, id, "")?;
    for spec in crate::import_collector::collect_imports(&program, &arena, &interner) {
        if !(spec.starts_with("runtime:") || spec.starts_with("std:") || spec == "core:intrinsics")
        {
            return Err(format!(
                "{id}: forbidden import \"{spec}\" — std may only import runtime:*/std:* or core:intrinsics"
            ));
        }
    }
    Ok(())
}

/// Compiles the entire stdlib directory into a single serialized VNB bytes buffer.
/// Sole producer of `.vnb`: `crates/varn-cli/build.rs` calls it to embed the
/// stdlib into `vn`, which then serves every host (CLI, LSP, isolates).
pub fn compile_stdlib_bundle(std_dir: &std::path::Path) -> Result<Vec<u8>, String> {
    #[derive(serde::Deserialize)]
    struct ManifestModule {
        id: String,
        #[serde(default)]
        pure: bool,
    }

    #[derive(serde::Deserialize)]
    struct Manifest {
        version: String,
        modules: Vec<ManifestModule>,
    }

    let manifest: Manifest = if let Ok(manifest_raw) = std::fs::read_to_string(std_dir.join("std.json")) {
        serde_json::from_str(&manifest_raw).map_err(|e| format!("invalid std.json: {e}"))?
    } else {
        let mut files = Vec::new();
        fn collect_vn(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        collect_vn(base, &p, out);
                    } else if p.extension().is_some_and(|e| e == "vn") {
                        if let Ok(rel) = p.strip_prefix(base) {
                            out.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
            }
        }
        collect_vn(std_dir, std_dir, &mut files);
        files.sort();
        let mut modules = Vec::new();
        for rel_path in files {
            let id = if rel_path.ends_with("/mod.vn") {
                let prefix = rel_path.strip_suffix("/mod.vn").unwrap();
                format!("std:{prefix}")
            } else if rel_path.ends_with(".vn") {
                let prefix = rel_path.strip_suffix(".vn").unwrap();
                format!("std:{prefix}")
            } else {
                continue;
            };
            let full_path = std_dir.join(&rel_path);
            let pure = if let Ok(source) = std::fs::read_to_string(&full_path) {
                !source.contains("\"runtime:") && !source.contains("'runtime:")
            } else {
                false
            };
            modules.push(ManifestModule { id, pure });
        }
        Manifest {
            version: "0.3.0".to_string(),
            modules,
        }
    };

    let mut modules = Vec::new();
    // Report every failing module in one pass: fixing the stdlib one build
    // round-trip at a time is not worth the four-minute rebuild.
    let mut failures = String::new();
    for m in &manifest.modules {
        let rel_id = m.id.strip_prefix("std:").ok_or("invalid std: prefix")?;
        let file = if std_dir.join(format!("{rel_id}/mod.vn")).exists() {
            std_dir.join(format!("{rel_id}/mod.vn"))
        } else {
            std_dir.join(format!("{rel_id}.vn"))
        };
        let source = std::fs::read_to_string(&file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;

        if let Err(e) = validate_imports(&m.id, &source) {
            failures.push_str(&format!("\n{e}"));
            continue;
        }

        let exports = crate::resolver::with_resolver(|r| r.stdlib_exports(&m.id));
        let bind = match crate::resolver::with_resolver(|r| r.stdlib_bind(&m.id)) {
            Some(b) => b,
            None => {
                let err_msg = match compile_source_checked(&source, &m.id) {
                    Ok(_) => "unknown bind failure".to_string(),
                    Err(e) => e,
                };
                return Err(format!("cannot bind {}: {}", m.id, err_msg));
            }
        };
        let shared_interner = crate::resolver::with_resolver(|r| r.interner_snapshot());
        let interface = crate::resolver::with_resolver(|r| {
            varn_checker::module_resolver::serialize_module_interface(
                &exports,
                &bind,
                &shared_interner,
                Some(r),
            )
        })
        .map_err(|e| format!("interface serialization failed for {}: {e}", m.id))?;

        let proto = match compile_source_checked(&source, &m.id) {
            Ok(p) => p,
            Err(e) => {
                failures.push_str(&format!("\n{e}"));
                continue;
            }
        };
        let bytecode = postcard::to_allocvec(&proto)
            .map_err(|e| format!("bytecode serialization failed for {}: {e}", m.id))?;

        modules.push(varn_modules::bundle::BundleModule {
            id: m.id.clone(),
            pure: m.pure,
            interface,
            bytecode,
            source,
        });
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    let bundle = varn_modules::bundle::StdBundle {
        std_version: manifest.version,
        host_api_version: varn_core::HOST_API_VERSION,
        modules,
    };

    Ok(varn_modules::bundle::write_bundle(&bundle))
}
