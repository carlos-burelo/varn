use super::super::ExportMap;
use super::cache_types::PortableModule;
use crate::binder::BindResult;
use crate::types::CheckerTyTable;
use std::path::PathBuf;
use std::sync::Arc;
use varn_core::AtomInterner;

pub(crate) struct CachedModule {
    pub(crate) exports: ExportMap,
    pub(crate) bind: BindResult,
}

pub fn serialize_module_interface(
    exports: &ExportMap,
    bind: &BindResult,
) -> Result<Vec<u8>, String> {
    let mut names = bind.interner.clone();
    names.absorb(exports.table.names());
    let mut table = (*exports.table).clone();
    table.absorb(&bind.ty_table);
    let cached = PortableModule::from_live(exports, bind, &names, &table);
    postcard::to_allocvec(&cached).map_err(|e| e.to_string())
}

pub fn deserialize_module_interface(bytes: &[u8]) -> Result<(ExportMap, BindResult), String> {
    let cached: PortableModule = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
    let (mut exports, bind) =
        cached.into_live(&mut AtomInterner::new(), Arc::new(CheckerTyTable::new()));
    super::super::exports::assign_slots(&mut exports);
    Ok((exports, bind))
}

pub(super) fn get_cache_dir(resolver: &super::super::resolver_disk::DiskResolver) -> PathBuf {
    resolver.types_cache_dir()
}

pub(super) use super::super::CarrierKind;

pub(super) fn cache_module_id(virtual_id: &str) -> varn_core::ModuleId {
    if virtual_id.starts_with("std:")
        || virtual_id.starts_with("core:")
        || virtual_id.starts_with("runtime:")
    {
        varn_core::ModuleId::stdlib(virtual_id)
    } else {
        varn_core::ModuleId::local_str(virtual_id)
    }
}

pub(super) fn cache_fingerprint(source: &str, carrier: CarrierKind) -> u64 {
    varn_modules::artifact::source_fingerprint(source) ^ ((carrier as u64) << 56)
}

pub(crate) fn try_load_cache(
    resolver: &super::super::resolver_disk::DiskResolver,
    virtual_id: &str,
    source: &str,
    carrier: CarrierKind,
) -> Option<CachedModule> {
    if virtual_id == "core:types/aliases" {
        return None;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source, carrier);
    let payload = varn_modules::artifact::read_module_artifact(
        &get_cache_dir(resolver),
        varn_modules::artifact::ArtifactKind::CheckerInterface,
        &id,
        fingerprint,
    )?;
    match deserialize_module_interface(&payload) {
        Ok((mut exports, mut bind)) => {
            let expected = varn_modules::canonical_or_original(std::path::Path::new(virtual_id));
            let got = bind.source_file.to_string();
            if got != virtual_id && got != expected {
                return None;
            }
            adopt_dependencies(resolver, virtual_id, &mut bind);
            exports.table = bind.ty_table.clone();
            Some(CachedModule { exports, bind })
        }
        Err(_) => None,
    }
}

fn adopt_dependencies(
    resolver: &super::super::resolver_disk::DiskResolver,
    virtual_id: &str,
    bind: &mut BindResult,
) {
    use super::super::ImportResolver;
    if !resolver.begin_cache_load(virtual_id) {
        return;
    }
    for dep in bind.deps.clone() {
        if resolver.is_binding(&dep) {
            continue;
        }
        let dep_bind = if super::super::paths::is_known_module(&dep) {
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
    resolver: &super::super::resolver_disk::DiskResolver,
    virtual_id: &str,
    source: &str,
    exports: &ExportMap,
    bind: &BindResult,
    carrier: CarrierKind,
) {
    if virtual_id == "core:types/aliases" {
        return;
    }
    let id = cache_module_id(virtual_id);
    let fingerprint = cache_fingerprint(source, carrier);
    if let Ok(payload) = serialize_module_interface(exports, bind) {
        varn_modules::artifact::write_module_artifact(
            &get_cache_dir(resolver),
            varn_modules::artifact::ArtifactKind::CheckerInterface,
            &id,
            fingerprint,
            &payload,
        );
    }
}
