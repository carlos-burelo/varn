use std::rc::Rc;
use std::sync::{Arc, Mutex};
use varn_checker::module_resolver::ImportResolver;

use rustc_hash::FxHashMap;
use varn_core::ModuleId;
use varn_modules::loader::ModuleLoader as CanonicalLoader;
use varn_types::FunctionProto;
use varn_vm::loader::{ModuleError, ModuleLoader};

pub struct PipelineLoader {
    registry: varn_modules::loader::ModuleRegistry,
    protos: Mutex<FxHashMap<String, (u64, Arc<[u8]>)>>,
    session: crate::resolver::Session,
}

impl PipelineLoader {
    pub fn new() -> Self {
        Self {
            registry: varn_modules::loader::default_registry(),
            protos: Mutex::new(FxHashMap::default()),
            session: crate::resolver::Session::new(),
        }
    }

    fn cached_proto(&self, key: &str, fingerprint: u64) -> Option<Rc<FunctionProto>> {
        let bytes = {
            let guard = self.protos.lock().ok()?;
            let (fp, bytes) = guard.get(key)?;
            if *fp != fingerprint {
                return None;
            }
            bytes.clone()
        };
        postcard::from_bytes::<FunctionProto>(&bytes)
            .ok()
            .map(Rc::new)
    }

    fn store_proto(&self, key: &str, fingerprint: u64, proto: &Rc<FunctionProto>) {
        if let Ok(bytes) = postcard::to_allocvec(proto.as_ref()) {
            if let Ok(mut guard) = self.protos.lock() {
                guard.insert(
                    key.to_owned(),
                    (fingerprint, Arc::from(bytes.into_boxed_slice())),
                );
            }
        }
    }
}

impl Default for PipelineLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleLoader for PipelineLoader {
    fn resolve(&self, specifier: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        self.registry
            .resolve(specifier, from)
            .map_err(|e| ModuleError::new(e.to_string()))
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        match id {
            ModuleId::Local(_) | ModuleId::Std(_) | ModuleId::Core(_) => {}
            _ => return Ok(None),
        }
        let source = CanonicalLoader::source(&self.registry, id)
            .map_err(|e| ModuleError::new(e.to_string()))?;
        if let Some(blob) = source.bytecode.as_ref() {
            return postcard::from_bytes(blob)
                .map(Rc::new)
                .map(Some)
                .map_err(|e| ModuleError::new(format!("corrupt bytecode for {id:?}: {e}")));
        }
        let text = source.text;
        let fingerprint = varn_modules::artifact::source_fingerprint(text.as_ref());
        let key = varn_modules::artifact::module_key(id, fingerprint);
        if let Some(hit) = self.cached_proto(&key, fingerprint) {
            return Ok(Some(hit));
        }
        let path = match id {
            ModuleId::Local(p) => p.as_ref(),
            ModuleId::Std(s) | ModuleId::Core(s) => s.as_ref(),
            _ => return Ok(None),
        };
        let proto = compile_source(text.as_ref(), path, &self.session)
            .map(Rc::new)
            .map_err(|e| ModuleError::new(format!("compile error in '{path}': {e}")))?;
        self.store_proto(&key, fingerprint, &proto);
        Ok(Some(proto))
    }
}

pub fn compile_source(
    source: &str,
    path: &str,
    session: &crate::resolver::Session,
) -> Result<FunctionProto, String> {
    compile_source_inner(source, path, false, session)
}

pub fn compile_source_checked(
    source: &str,
    path: &str,
    session: &crate::resolver::Session,
) -> Result<FunctionProto, String> {
    compile_source_inner(source, path, true, session)
}

fn compile_source_inner(
    source: &str,
    path: &str,
    reject_type_errors: bool,
    session: &crate::resolver::Session,
) -> Result<FunctionProto, String> {
    let (program, arena, interner) = crate::quiet_parse::parse_module(source, path, "")?;
    let check = varn_checker::Checker::check(&program, &arena, interner, session.resolver());
    if reject_type_errors && check.diagnostics.has_errors() {
        let mut msg = String::new();
        for d in check.diagnostics.errors() {
            msg.push_str(&format!(
                "\n  {path}:{}:{}: {}",
                d.range.start.line, d.range.start.column, d.message
            ));
        }
        return Err(format!("type errors in stdlib module:{msg}"));
    }
    let resolver = session.resolver();
    let exports =
        if path.starts_with("std:") || path.starts_with("core:") || path.starts_with("runtime:") {
            resolver.stdlib_exports(path)
        } else {
            resolver.module_exports(path, &mut vec![])
        };
    let export_names = crate::compile::sorted_export_names(&exports);
    let (result, _) =
        crate::compile::emit_and_compile(&program, &arena, &check, export_names, source, false);
    result
}

fn validate_imports(id: &str, source: &str) -> Result<(), String> {
    let (program, arena, interner) = crate::quiet_parse::parse_only(source, id, "")?;
    for spec in crate::import_collector::collect_imports(&program, &arena, &interner) {
        varn_modules::layer::check_import(varn_modules::layer::Layer::Std, &spec)
            .map_err(|message| format!("{id}: {message}"))?;
    }
    Ok(())
}

pub fn compile_stdlib_bundle(
    std_dir: &std::path::Path,
    session: &crate::resolver::Session,
) -> Result<Vec<u8>, String> {
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

    let manifest: Manifest =
        if let Ok(manifest_raw) = std::fs::read_to_string(std_dir.join("std.json")) {
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

        let resolver = session.resolver();
        let exports = resolver.stdlib_exports(&m.id);
        let bind = match resolver.stdlib_bind(&m.id) {
            Some(b) => b,
            None => {
                let err_msg = match compile_source_checked(&source, &m.id, session) {
                    Ok(_) => "unknown bind failure".to_string(),
                    Err(e) => e,
                };
                return Err(format!("cannot bind {}: {}", m.id, err_msg));
            }
        };
        let interface = varn_checker::module_resolver::serialize_module_interface(&exports, &bind)
            .map_err(|e| format!("interface serialization failed for {}: {e}", m.id))?;

        let proto = match compile_source_checked(&source, &m.id, session) {
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
