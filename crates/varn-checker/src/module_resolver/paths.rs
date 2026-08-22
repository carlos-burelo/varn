use rustc_hash::FxHashMap;
use std::path::Path;

pub fn is_known_module(specifier: &str) -> bool {
    varn_modules::resolver::is_known_module(specifier)
}

pub fn resolve_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    let base_str = base_dir.to_string_lossy().into_owned();
    let cached = super::store::RESOLVED_PATH_CACHE.with(|c| {
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

    let res = varn_modules::resolver::resolve_specifier_path(base_dir, specifier)?;

    super::store::RESOLVED_PATH_CACHE.with(|c| {
        let mut guard = c.borrow_mut();
        let cache = guard.get_or_insert_with(FxHashMap::default);
        cache.insert((base_str.clone(), specifier.to_owned()), res.clone());
    });
    Some(res)
}

pub fn resolve_package_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    varn_modules::resolve_pkg_specifier(base_dir, specifier)
}

pub(super) fn resolve_relative(base_dir: &Path, specifier: &str) -> String {
    let raw = match varn_core::ImportSpecifier::parse(specifier) {
        varn_core::ImportSpecifier::Package(_) => {
            resolve_package_specifier_path(base_dir, specifier)
                .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned())
        }
        _ => resolve_specifier_path(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
    };
    varn_modules::canonical_or_original(Path::new(&raw))
}
