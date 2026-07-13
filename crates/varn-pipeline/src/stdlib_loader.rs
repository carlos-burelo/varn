use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;
use varn_core::{ImportSpecifier, ModuleId};
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

static COMPILED_BYTES: Mutex<Option<FxHashMap<String, (u64, Arc<[u8]>)>>> = Mutex::new(None);

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
        c.borrow_mut().insert(key.to_owned(), (fingerprint, proto.clone()));
    });
    Some(proto)
}

fn store_proto(key: &str, fingerprint: u64, proto: &Rc<FunctionProto>) {
    if let Ok(bytes) = postcard::to_allocvec(proto.as_ref()) {
        if let Ok(mut guard) = COMPILED_BYTES.lock() {
            guard
                .get_or_insert_with(FxHashMap::default)
                .insert(key.to_owned(), (fingerprint, Arc::from(bytes.into_boxed_slice())));
        }
    }
    PROTO_CACHE.with(|c| {
        c.borrow_mut().insert(key.to_owned(), (fingerprint, proto.clone()));
    });
}

pub struct FileLoader;

impl ModuleLoader for FileLoader {
    fn resolve(&self, spec: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        match ImportSpecifier::parse(spec) {
            ImportSpecifier::Relative(_) | ImportSpecifier::Package(_) => {
                varn_modules::resolver::ModuleResolver::new()
                    .resolve(spec, from)
                    .map_err(ModuleError::new)
            }
            _ => Err(ModuleError::new(format!(
                "FileLoader cannot resolve non-local specifier: {spec}"
            ))),
        }
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        let path = match id {
            ModuleId::Local(p) => p.as_ref(),
            _ => return Ok(None),
        };
        let source = std::fs::read_to_string(path)
            .map_err(|e| ModuleError::new(format!("cannot read '{path}': {e}")))?;
        let fingerprint = source_fingerprint(&source);
        if let Some(hit) = cached_proto(path, fingerprint) {
            return Ok(Some(hit));
        }
        let proto = compile_source(&source, path)
            .map(Rc::new)
            .map_err(|e| ModuleError::new(format!("compile error in '{path}': {e}")))?;
        store_proto(path, fingerprint, &proto);
        Ok(Some(proto))
    }

    fn native(&self, _id: &ModuleId) -> Option<varn_types::Value> {
        None
    }
}

pub struct StdlibLoader;

impl ModuleLoader for StdlibLoader {
    fn resolve(&self, specifier: &str, _from: &ModuleId) -> Result<ModuleId, ModuleError> {
        match ImportSpecifier::parse(specifier) {
            ImportSpecifier::Stdlib(s) => Ok(ModuleId::Std(s)),
            ImportSpecifier::Core(s) => Ok(ModuleId::Core(s)),
            ImportSpecifier::Runtime(s) => Ok(ModuleId::Runtime(s)),
            _ => Err(ModuleError::new(format!(
                "StdlibLoader cannot resolve non-stdlib specifier: {specifier}"
            ))),
        }
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

        if let Some(hit) = cached_proto(spec, STD_FINGERPRINT) {
            return Ok(Some(hit));
        }
        let proto = Rc::new(load_uncached(spec)?);
        store_proto(spec, STD_FINGERPRINT, &proto);
        Ok(Some(proto))
    }
}

fn load_uncached(spec: &str) -> Result<FunctionProto, ModuleError> {
    let provider = varn_modules::provider::get()
        .ok_or_else(|| ModuleError::new("stdlib provider not registered"))?;

    if let Some(blob) = provider.bytecode_blob(spec) {
        return postcard::from_bytes(blob).map_err(|e| {
            ModuleError::new(format!("corrupt std bundle bytecode for {spec}: {e}"))
        });
    }

    let source = provider
        .embedded_source(spec)
        .map(|s| s.to_owned())
        .or_else(|| {
            provider
                .source_path(spec)
                .and_then(|p| std::fs::read_to_string(p).ok())
        })
        .ok_or_else(|| ModuleError::new(format!("stdlib source not found: {spec}")))?;

    compile_source(&source, spec)
        .map_err(|e| ModuleError::new(format!("stdlib compile error in {spec}: {e}")))
}

/// Compile one stdlib-namespace module source to a FunctionProto. Used by StdlibLoader and xtask build-std.
pub fn compile_source(source: &str, path: &str) -> Result<FunctionProto, String> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, path);
    let mut program =
        varn_parser::parse(tokens, lexeme_buf, path).map_err(|errs| errs[0].message.clone())?;
    varn_core::assign_ast_ids(&mut program);
    let check = varn_checker::Checker::check(&program);
    let exports =
        if path.starts_with("std:") || path.starts_with("core:") || path.starts_with("runtime:") {
            varn_checker::module_resolver::resolve_stdlib_module_exports_ref(path)
        } else {
            varn_checker::module_resolver::resolve_module_exports_ref(path, &mut vec![])
        };
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();
    varn_opt::compile_module(
        &program,
        &check.type_annotations,
        &check.extension_calls,
        &check.extension_members,
        &check.extension_set_members,
        export_names,
    )
    .map_err(|e| e.to_string())
}
