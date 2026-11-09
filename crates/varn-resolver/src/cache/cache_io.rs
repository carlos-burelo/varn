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

fn cache_dir_of(resolver: &super::super::module_resolver::ModuleResolver) -> PathBuf {
    get_cache_dir(resolver)
}

fn read_manifest(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    source_digest: u64,
) -> Option<Vec<String>> {
    let payload = varn_modules::artifact::read_module_artifact(
        &cache_dir_of(resolver),
        varn_modules::artifact::ArtifactKind::ImportManifest,
        &cache_module_id(virtual_id),
        source_digest,
    )?;
    postcard::from_bytes(&payload).ok()
}

fn write_manifest(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    source_digest: u64,
    deps: &[String],
) {
    if let Ok(payload) = postcard::to_allocvec(deps) {
        varn_modules::artifact::write_module_artifact(
            &cache_dir_of(resolver),
            varn_modules::artifact::ArtifactKind::ImportManifest,
            &cache_module_id(virtual_id),
            source_digest,
            &payload,
        );
    }
}

fn dependency_digest(
    resolver: &super::super::module_resolver::ModuleResolver,
    dep: &str,
    visiting: &mut Vec<String>,
) -> Option<u64> {
    if varn_core::ModuleKind::Core.owns(dep) || varn_core::ModuleKind::Runtime.owns(dep) {
        return Some(0);
    }
    let image = varn_modules::provider::get().map_or(0, |p| p.image_identity());
    if image != 0 && varn_core::ModuleKind::Stdlib.owns(dep) {
        return Some(image);
    }
    if visiting.iter().any(|open| open == dep) {
        return Some(0);
    }
    let text = varn_modules::loader::read_module_text(dep).ok()?;
    let digest = cache_fingerprint(&text);
    let deps = read_manifest(resolver, dep, digest)?;
    visiting.push(dep.to_owned());
    let key = interface_key(resolver, digest, &deps, visiting);
    visiting.pop();
    key
}

fn interface_key(
    resolver: &super::super::module_resolver::ModuleResolver,
    source_digest: u64,
    deps: &[String],
    visiting: &mut Vec<String>,
) -> Option<u64> {
    let mut sorted = deps.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut stamp = source_digest.to_le_bytes().to_vec();
    for dep in sorted {
        let digest = dependency_digest(resolver, &dep, visiting)?;
        stamp.extend_from_slice(dep.as_bytes());
        stamp.extend_from_slice(&digest.to_le_bytes());
    }
    Some(varn_core::fingerprint::fingerprint(&stamp))
}

pub(crate) fn try_load_cache(
    resolver: &super::super::module_resolver::ModuleResolver,
    virtual_id: &str,
    source: &str,
) -> Option<CachedModule> {
    if !has_portable_interface(virtual_id) {
        return None;
    }
    let digest = cache_fingerprint(source);
    let deps = read_manifest(resolver, virtual_id, digest)?;
    let key = interface_key(resolver, digest, &deps, &mut vec![virtual_id.to_owned()])?;
    let interface = varn_modules::artifact::read_module_artifact(
        &cache_dir_of(resolver),
        varn_modules::artifact::ArtifactKind::CheckerInterface,
        &cache_module_id(virtual_id),
        key,
    )?;
    adopt_interface(resolver, virtual_id, &interface)
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
    use varn_sem::resolver::ImportResolver;
    if !has_portable_interface(virtual_id) {
        return;
    }
    let digest = cache_fingerprint(source);
    let deps: Vec<String> = bind.deps.iter().map(ToString::to_string).collect();
    for dep in &deps {
        if varn_core::ModuleKind::of(dep).is_none() {
            let _ = resolver.module_bind(dep);
        } else if varn_core::ModuleKind::Stdlib.owns(dep) {
            let _ = resolver.stdlib_bind(dep);
        }
    }
    write_manifest(resolver, virtual_id, digest, &deps);
    let Some(key) = interface_key(resolver, digest, &deps, &mut vec![virtual_id.to_owned()]) else {
        return;
    };
    if let Ok(interface) = serialize_module_interface(exports, bind) {
        varn_modules::artifact::write_module_artifact(
            &cache_dir_of(resolver),
            varn_modules::artifact::ArtifactKind::CheckerInterface,
            &cache_module_id(virtual_id),
            key,
            &interface,
        );
    }
}
