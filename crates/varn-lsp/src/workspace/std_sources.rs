












use std::path::PathBuf;

const DIR_NAME: &str = "std-src";


pub fn root() -> PathBuf {
    varn_core::paths::varn_home_dir()
        .join(DIR_NAME)
        .join(format!("{:08x}", varn_modules::artifact::BUILD_FINGERPRINT))
}


fn path_for_specifier(specifier: &str) -> Option<PathBuf> {
    let (kind, name) = specifier.split_once(':')?;
    Some(root().join(kind).join(format!("{name}.vn")))
}






fn carried_source(
    provider: &dyn varn_modules::provider::StdlibProvider,
    specifier: &str,
) -> Option<&'static str> {
    provider
        .bundled_source(specifier)
        .or_else(|| provider.embedded_source(specifier))
}







pub fn materialize() {
    let Some(provider) = varn_modules::provider::get() else {
        return;
    };
    for spec in provider.all_specs() {
        let Some(source) = carried_source(provider, spec.id) else {
            continue;
        };
        let Some(path) = path_for_specifier(spec.id) else {
            continue;
        };
        
        
        if std::fs::read_to_string(&path).is_ok_and(|existing| existing == source) {
            continue;
        }
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
        }
        let _ = std::fs::write(&path, source);
    }
}


pub fn path_for(specifier: &str) -> Option<PathBuf> {
    let path = path_for_specifier(specifier)?;
    path.is_file().then_some(path)
}








pub fn specifier_from_path(path: &str) -> Option<String> {
    let normalized = varn_modules::resolver::normalize_display_path(path);

    if let Some(rest) = strip_root(&normalized, &root()) {
        let (kind, file) = rest.split_once('/')?;
        return Some(format!("{kind}:{}", file.strip_suffix(".vn")?));
    }

    if let (varn_modules::std_root::StdSource::SourceTree(tree), _) =
        varn_modules::std_root::resolve()
    {
        if let Some(rest) = strip_root(&normalized, &tree) {
            if !rest.contains('/') {
                return Some(format!(
                    "{}{}",
                    varn_modules::spec::STD_PREFIX,
                    rest.strip_suffix(".vn")?
                ));
            }
        }
    }

    None
}

fn strip_root(normalized_path: &str, root: &std::path::Path) -> Option<String> {
    let root = varn_modules::resolver::normalize_display_path(&root.to_string_lossy());
    Some(
        normalized_path
            .strip_prefix(&root)?
            .trim_start_matches('/')
            .to_owned(),
    )
}



pub fn is_mirrored_uri(uri: &str) -> bool {
    let path =
        varn_modules::resolver::normalize_display_path(&varn_modules::resolver::uri_to_path(uri));
    let root = varn_modules::resolver::normalize_display_path(&root().to_string_lossy());
    path.starts_with(&root)
}






pub fn resolve_module_file(specifier: &str) -> Option<PathBuf> {
    let provider = varn_modules::provider::get()?;
    let path = provider
        .source_path(specifier)
        .filter(|p| p.is_file())
        .or_else(|| path_for(specifier))?;
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}
