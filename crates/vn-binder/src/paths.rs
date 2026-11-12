use std::path::Path;

pub fn is_known_module(specifier: &str) -> bool {
    vn_modules::resolver::is_known_module(specifier)
}

pub fn resolve_package_specifier_path(base_dir: &Path, specifier: &str) -> Result<String, String> {
    vn_modules::resolve_pkg_specifier(base_dir, specifier)
}

pub fn resolve_relative(
    resolver: &dyn vn_sem::resolver::ImportResolver,
    base_dir: &Path,
    specifier: &str,
) -> String {
    let raw = match vn_core::ImportSpecifier::parse(specifier) {
        vn_core::ImportSpecifier::Package(_) => {
            resolve_package_specifier_path(base_dir, specifier)
                .unwrap_or_else(|_| base_dir.join(specifier).to_string_lossy().into_owned())
        }
        vn_core::ImportSpecifier::Relative(_)
        | vn_core::ImportSpecifier::Stdlib(_)
        | vn_core::ImportSpecifier::Core(_)
        | vn_core::ImportSpecifier::Runtime(_) => resolver
            .resolve_specifier(base_dir, specifier)
            .unwrap_or_else(|| base_dir.join(specifier).to_string_lossy().into_owned()),
    };
    vn_modules::canonical_or_original(Path::new(&raw))
}
