use crate::binder::BindResult;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub type ExportMap = FxHashMap<String, crate::symbol::Symbol>;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct CachedModule {
    pub exports: ExportMap,
    pub bind: BindResult,
}

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
    super::exports::assign_slots(&mut cached.exports);
    Ok((cached.exports, cached.bind))
}

pub(super) fn compute_source_hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn get_cache_dir() -> PathBuf {
    super::store::PROJECT_ROOT.with(|r| {
        let mut guard = r.borrow_mut();
        if guard.is_none() {
            let current_dir =
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = varn_modules::artifact::find_project_root(&current_dir);
            *guard = Some(root);
        }
        varn_modules::artifact::get_types_cache_dir(guard.as_ref().unwrap())
    })
}

pub(super) fn try_load_cache(virtual_id: &str, source: &str) -> Option<CachedModule> {
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
            super::exports::assign_slots(&mut val.exports);
            Some(val)
        }
        Err(_) => None,
    }
}

pub(super) fn save_to_cache(virtual_id: &str, source: &str, exports: &ExportMap, bind: &BindResult) {
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
