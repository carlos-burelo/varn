use crate::binder::BindResult;
use crate::module_resolver::cache::{deserialize_module_interface, ExportMap};
use crate::module_resolver::store::{
    bind_cache_get, bind_cache_insert, export_cache_get, export_cache_insert,
};
use rustc_hash::FxHashMap;
use std::path::Path;
use std::rc::Rc;
use varn_core::ModuleId;

pub fn resolve_stdlib_module_exports(specifier: &str) -> ExportMap {
    resolve_stdlib_module_exports_ref(specifier)
        .as_ref()
        .clone()
}

pub(super) enum Carrier {
    Blob,
    Embedded(&'static str),
    File(String),
}

pub(super) fn stdlib_carrier(specifier: &str) -> Option<Carrier> {
    let provider = varn_modules::provider::get()?;

    if specifier != "std:types" && provider.interface_blob(specifier).is_some() {
        return Some(Carrier::Blob);
    }
    if let Some(source) = provider
        .embedded_source(specifier)
        .or_else(|| provider.bundled_source(specifier))
    {
        return Some(Carrier::Embedded(source));
    }
    provider
        .source_path(specifier)
        .map(|p| Carrier::File(p.to_string_lossy().into_owned()))
}

pub fn resolve_stdlib_module_exports_ref(specifier: &str) -> Rc<ExportMap> {
    let id = ModuleId::stdlib(specifier);
    let key = id.as_str();

    if let Some(cached) = export_cache_get(&key) {
        return cached;
    }

    let result = match stdlib_carrier(specifier) {
        Some(Carrier::Blob) => resolve_from_interface_blob(specifier, &key).map(|(e, _)| e),
        Some(Carrier::Embedded(source)) => {
            let mut visiting = vec![];
            Some(resolve_from_embedded_source(
                specifier,
                source,
                &mut visiting,
            ))
        }
        Some(Carrier::File(abs)) => {
            let mut visiting = vec![];
            Some(super::resolve_module_exports_ref(&abs, &mut visiting))
        }
        None => None,
    };

    match result {
        Some(exports) => {
            export_cache_insert(key.to_string(), Rc::clone(&exports));
            exports
        }
        None => Rc::new(FxHashMap::default()),
    }
}

pub fn resolve_stdlib_module_bind_ref(specifier: &str) -> Option<Rc<BindResult>> {
    let id = ModuleId::stdlib(specifier);
    let key = id.as_str();

    if let Some(cached) = bind_cache_get(&key) {
        return Some(cached);
    }

    match stdlib_carrier(specifier)? {
        Carrier::Blob => resolve_from_interface_blob(specifier, &key).map(|(_, bind)| bind),
        Carrier::Embedded(source) => bind_from_embedded_source(specifier, source),
        Carrier::File(abs) => super::cache_get_or_insert_ref(&abs),
    }
}

pub(super) fn resolve_from_interface_blob(
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

pub(super) fn resolve_from_embedded_source(
    virtual_id: &str,
    source: &str,
    visiting: &mut Vec<String>,
) -> Rc<ExportMap> {
    if visiting.iter().any(|v| v == virtual_id) {
        return Rc::new(FxHashMap::default());
    }
    visiting.push(virtual_id.to_owned());

    if let Some(cached) = super::cache::try_load_cache(virtual_id, source) {
        let bind_rc = Rc::new(cached.bind);
        let exports_rc = Rc::new(cached.exports);

        bind_cache_insert(virtual_id.to_owned(), Rc::clone(&bind_rc));
        visiting.pop();
        return exports_rc;
    }

    let Some((program, _lex_errs)) = super::parse_and_cache(source, virtual_id) else {
        visiting.pop();
        return Rc::new(FxHashMap::default());
    };

    let bind = super::bind_and_cache(&program, Vec::new(), virtual_id);

    let base_dir = Path::new(".");
    let mut exports = ExportMap::default();
    super::exports::collect_exports(
        &program.body,
        bind.as_ref(),
        virtual_id,
        base_dir,
        visiting,
        &mut exports,
    );
    super::exports::assign_slots(&mut exports);

    super::cache::save_to_cache(virtual_id, source, &exports, bind.as_ref());

    visiting.pop();
    Rc::new(exports)
}

pub(super) fn bind_from_embedded_source(virtual_id: &str, source: &str) -> Option<Rc<BindResult>> {
    if let Some(cached) = bind_cache_get(virtual_id) {
        return Some(cached);
    }
    if let Some(cached) = super::cache::try_load_cache(virtual_id, source) {
        let bind_rc = Rc::new(cached.bind);
        bind_cache_insert(virtual_id.to_owned(), Rc::clone(&bind_rc));
        let exports_rc = Rc::new(cached.exports);
        let id = ModuleId::stdlib(virtual_id);
        export_cache_insert(id.as_str().to_owned(), exports_rc);
        return Some(bind_rc);
    }
    let (program, lex_errs) = super::parse_and_cache(source, virtual_id)?;
    Some(super::bind_and_cache(&program, lex_errs, virtual_id))
}
