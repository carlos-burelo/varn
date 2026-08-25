use std::path::Path;

pub fn is_known_module(specifier: &str) -> bool {
    varn_modules::resolver::is_known_module(specifier)
}

pub fn resolve_package_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    varn_modules::resolve_pkg_specifier(base_dir, specifier)
}

pub(super) fn resolve_relative(
    resolver: &dyn super::ImportResolver,
    base_dir: &Path,
    specifier: &str,
) -> String {
    let raw = match varn_core::ImportSpecifier::parse(specifier) {
        varn_core::ImportSpecifier::Package(_) => {
            resolve_package_specifier_path(base_dir, specifier)
                .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned())
        }
        _ => resolver
            .resolve_specifier(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
    };
    varn_modules::canonical_or_original(Path::new(&raw))
}
