use std::path::PathBuf;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::codec::{
    deserialize_module_interface, has_portable_interface, serialize_module_interface,
};
use varn_sem::exports::ExportMap;

pub(crate) struct CachedModule {
    pub(crate) exports: ExportMap,
    pub(crate) bind: BindResult,
}

pub(super) fn get_cache_dir(resolver: &super::super::module_resolver::ModuleResolver) -> PathBuf {
    resolver.types_cache_dir()
}

pub(super) fn cache_module_id(virtual_id: &str) -> varn_core::ModuleId {
    if varn_core::ModuleKind::of(virtual_id).is_some() {
        varn_core::ModuleId::stdlib(virtual_id)
    } else {
        varn_core::ModuleId::local_str(virtual_id)
    }
}

pub(super) fn cache_fingerprint(source: &str) -> u64 {
    varn_core::fingerprint::fingerprint(source.as_bytes())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedInterfaceEnvelope {
    interface: Vec<u8>,
    dep_hashes: Vec<(String, u64)>,
}

fn dep_source_fingerprint(dep: &str) -> Option<u64> {
    let text = varn_modules::loader::read_module_text(dep).ok()?;
    Some(varn_core::fingerprint::fingerprint(text.as_bytes()))
}

fn current_dep_hashes(bind: &BindResult) -> Option<Vec<(String, u64)>> {
    bind.deps
        .iter()
        .map(|dep| {
            let dep = dep.to_string();
            dep_source_fingerprint(&dep).map(|h| (dep, h))
        })
        .collect()
}

pub(crate) fn try_load_cache(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    source: &str,
) -> Option<CachedModule> {
    if !has_portable_interface(virtual_id) {
        return None;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source);
    let payload = varn_modules::artifact::read_module_artifact(
        &get_cache_dir(resolver),
        varn_modules::artifact::ArtifactKind::CheckerInterface,
        &id,
        fingerprint,
    )?;
    let envelope: CachedInterfaceEnvelope = postcard::from_bytes(&payload).ok()?;
    for (dep, saved_hash) in &envelope.dep_hashes {
        if dep_source_fingerprint(dep) != Some(*saved_hash) {
            return None;
        }
    }
    adopt_interface(resolver, virtual_id, &envelope.interface)
}

pub(crate) fn adopt_interface(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    interface: &[u8],
) -> Option<CachedModule> {
    if !has_portable_interface(virtual_id) {
        return None;
    }
    let (mut exports, mut bind) = deserialize_module_interface(interface).ok()?;
    let expected = varn_modules::canonical_or_original(std::path::Path::new(virtual_id));
    let got = bind.source_file.to_string();
    if got != virtual_id && got != expected {
        return None;
    }
    adopt_dependencies(resolver, virtual_id, &mut bind);
    exports.table = bind.ty_table.clone();
    Some(CachedModule { exports, bind })
}

fn adopt_dependencies(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    bind: &mut BindResult,
) {
    use varn_sem::resolver::ImportResolver;
    if !resolver.begin_cache_load(virtual_id) {
        return;
    }
    for dep in bind.deps.clone() {
        if resolver.is_binding(&dep) {
            continue;
        }
        let dep_bind = if varn_binder::paths::is_known_module(&dep) {
            resolver.stdlib_bind(&dep)
        } else {
            resolver.module_bind(&dep)
        };
        if let Some(dep_bind) = dep_bind {
            bind.interner.absorb(dep_bind.ty_table.names());
            Arc::make_mut(&mut bind.ty_table).absorb(&dep_bind.ty_table);
        }
    }
    resolver.end_cache_load(virtual_id);
}

pub(crate) fn save_to_cache(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    source: &str,
    exports: &ExportMap,
    bind: &BindResult,
) {
    if !has_portable_interface(virtual_id) {
        return;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source);
    if let Ok(payload) = serialize_module_interface(exports, bind) {
        let Some(dep_hashes) = current_dep_hashes(bind) else {
            return;
        };
        let envelope = CachedInterfaceEnvelope {
            interface: payload,
            dep_hashes,
        };
        if let Ok(wrapped) = postcard::to_allocvec(&envelope) {
            varn_modules::artifact::write_module_artifact(
                &get_cache_dir(resolver),
                varn_modules::artifact::ArtifactKind::CheckerInterface,
                &id,
                fingerprint,
                &wrapped,
            );
        }
    }
}
