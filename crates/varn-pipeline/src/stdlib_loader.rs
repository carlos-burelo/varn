use std::rc::Rc;
use std::sync::{Arc, Mutex};
use varn_sem::resolver::ImportResolver;

use rustc_hash::FxHashMap;
use varn_core::ModuleId;
use varn_modules::loader::ModuleLoader as CanonicalLoader;
use varn_types::FunctionProto;
use varn_vm::loader::{ModuleError, ModuleLoader};

pub struct PipelineLoader {
    registry: varn_modules::loader::ModuleRegistry,
    protos: Mutex<ProtoBytesCache>,
    session: crate::resolver::Session,
}

type ProtoBytesCache = FxHashMap<String, (u64, Arc<[u8]>)>;

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
        if let Ok(bytes) = postcard::to_allocvec(proto.as_ref())
            && let Ok(mut guard) = self.protos.lock()
        {
            guard.insert(
                key.to_owned(),
                (fingerprint, Arc::from(bytes.into_boxed_slice())),
            );
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
            ModuleId::Runtime(_) | ModuleId::Package { .. } => return Ok(None),
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
        let fingerprint = varn_core::fingerprint::fingerprint(text.as_bytes());
        let key = varn_modules::artifact::module_key(id, fingerprint);
        if let Some(hit) = self.cached_proto(&key, fingerprint) {
            return Ok(Some(hit));
        }
        let path = match id {
            ModuleId::Local(p) => p.as_ref(),
            ModuleId::Std(s) | ModuleId::Core(s) => s.as_ref(),
            ModuleId::Runtime(_) | ModuleId::Package { .. } => return Ok(None),
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
    crate::compile::report_export_collisions(&exports, path)?;
    let mut tir = crate::compile::emit_tir(&program, &arena, &check);
    crate::layer_link::verify_synthetic_imports(
        path,
        &program,
        &arena,
        &check.bind.interner,
        crate::compile::tir_import_sources(&tir)
            .iter()
            .map(|s| s.as_ref()),
    )?;
    crate::compile::inject_star_reexports(&mut tir, &exports);
    crate::compile::resolve_import_slots(&mut tir, session.resolver(), path);
    let (result, _) = crate::compile::compile_tir(&tir, export_names, source, false);
    result
}

fn std_imports_are_pure(id: &str, source: &str) -> Result<bool, String> {
    let (program, arena, interner) = crate::quiet_parse::parse_only(source, id, "")?;
    let mut pure = true;
    for spec in crate::import_collector::collect_imports(&program, &arena, &interner) {
        varn_modules::layer::check_import(varn_modules::layer::Layer::Std, &spec)
            .map_err(|message| format!("{id}: {message}"))?;
        pure &= !spec.starts_with(varn_modules::RUNTIME_PREFIX);
    }
    Ok(pure)
}

fn std_tree_modules(std_dir: &std::path::Path) -> Vec<(String, String)> {
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if p.extension().is_some_and(|e| e == "vn")
                && let Ok(rel) = p.strip_prefix(base)
            {
                let rel = rel.to_string_lossy().replace('\\', "/");
                let stem = rel
                    .strip_suffix("/mod.vn")
                    .or_else(|| rel.strip_suffix(".vn"))
                    .unwrap_or(&rel);
                out.push((format!("std:{stem}"), rel.clone()));
            }
        }
    }
    let mut out = Vec::new();
    walk(std_dir, std_dir, &mut out);
    out.sort();
    out
}

struct ImageUnit {
    id: String,
    path: String,
    source: String,
    pure: Option<bool>,
}

fn core_units() -> Result<Vec<ImageUnit>, String> {
    let provider =
        varn_modules::provider::get().ok_or_else(|| "stdlib provider not registered".to_owned())?;
    varn_modules::core_module_ids()
        .into_iter()
        .map(|id| {
            let spec = provider
                .spec_for(id)
                .ok_or_else(|| format!("core module without spec: {id}"))?;
            let source = provider
                .embedded_source(id)
                .ok_or_else(|| format!("core module without source: {id}"))?;
            Ok(ImageUnit {
                id: id.to_owned(),
                path: spec.vn_source.to_owned(),
                source: source.to_owned(),
                pure: Some(spec.pure),
            })
        })
        .collect()
}

fn std_units(std_dir: &std::path::Path) -> Result<Vec<ImageUnit>, String> {
    std_tree_modules(std_dir)
        .into_iter()
        .map(|(id, path)| {
            let file = std_dir.join(&path);
            let source = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            Ok(ImageUnit {
                id,
                path,
                source,
                pure: None,
            })
        })
        .collect()
}

pub fn compile_stdlib_image(
    std_dir: &std::path::Path,
    session: &crate::resolver::Session,
) -> Result<Vec<u8>, String> {
    struct Compiled {
        unit: ImageUnit,
        pure: bool,
        interface: Vec<u8>,
        bytecode: Vec<u8>,
    }

    let mut units = core_units()?;
    units.extend(std_units(std_dir)?);

    let mut compiled = Vec::new();
    let mut failures = String::new();
    for unit in units {
        let id = unit.id.as_str();
        let pure = match unit.pure {
            Some(pure) => pure,
            None => match std_imports_are_pure(id, &unit.source) {
                Ok(pure) => pure,
                Err(e) => {
                    failures.push_str(&format!("\n{e}"));
                    continue;
                }
            },
        };

        let resolver = session.resolver();
        let interface = if varn_sem::codec::has_portable_interface(id) {
            let exports = resolver.stdlib_exports(id);
            let Some(bind) = resolver.stdlib_bind(id) else {
                let err_msg = match compile_source_checked(&unit.source, id, session) {
                    Ok(_) => "unknown bind failure".to_string(),
                    Err(e) => e,
                };
                return Err(format!("cannot bind {id}: {err_msg}"));
            };
            varn_sem::codec::serialize_module_interface(&exports, &bind)
                .map_err(|e| format!("interface serialization failed for {id}: {e}"))?
        } else {
            Vec::new()
        };

        let proto = match compile_source_checked(&unit.source, id, session) {
            Ok(p) => p,
            Err(e) => {
                failures.push_str(&format!("\n{e}"));
                continue;
            }
        };
        let bytecode = postcard::to_allocvec(&proto)
            .map_err(|e| format!("bytecode serialization failed for {id}: {e}"))?;

        compiled.push(Compiled {
            unit,
            pure,
            interface,
            bytecode,
        });
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    let modules = compiled
        .iter()
        .map(|m| varn_modules::std_image::ImageModule {
            id: &m.unit.id,
            path: &m.unit.path,
            pure: m.pure,
            interface: &m.interface,
            bytecode: &m.bytecode,
            source: &m.unit.source,
        })
        .collect();
    Ok(varn_modules::std_image::write_image(
        env!("CARGO_PKG_VERSION"),
        varn_core::HOST_API_VERSION,
        modules,
    ))
}
