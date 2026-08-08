//! On-disk mirror of the stdlib the running binary carries.
//!
//! Editor navigation needs a `file://` URI — goto-definition, hover and the
//! symbol index all end up opening a document — but a released `vn` has no
//! std tree on disk to point at: `std:` lives in the embedded bundle and
//! `core:`/`runtime:` live in `include_str!` constants. Writing them out once
//! gives every LSP client real files with no client-side scheme handling,
//! which is what `varn://` would have required from each editor separately.
//! Same trade Rust makes shipping `rust-src` into the sysroot.
//!
//! Keyed by `BUILD_FINGERPRINT`: a rebuilt toolchain gets a fresh directory
//! rather than serving stale text next to freshly compiled bytecode.

use std::path::PathBuf;

const DIR_NAME: &str = "std-src";

/// `<VARN_HOME>/std-src/<fingerprint>/`.
pub fn root() -> PathBuf {
    varn_core::paths::varn_home_dir()
        .join(DIR_NAME)
        .join(format!("{:08x}", varn_modules::artifact::BUILD_FINGERPRINT))
}

/// `std:math` → `<root>/std/math.vn`. `None` for ids without a category.
fn path_for_specifier(specifier: &str) -> Option<PathBuf> {
    let (kind, name) = specifier.split_once(':')?;
    Some(root().join(kind).join(format!("{name}.vn")))
}

/// Source text this binary carries for `specifier`, from either storage form.
///
/// `bundled_source` rather than `embedded_source` for `std:`: the latter
/// means "compile this module from this text" and drives the recompilation
/// graph, which blob-backed modules must stay out of.
fn carried_source(
    provider: &dyn varn_modules::provider::StdlibProvider,
    specifier: &str,
) -> Option<&'static str> {
    provider
        .bundled_source(specifier)
        .or_else(|| provider.embedded_source(specifier))
}

/// Writes out every module whose source this binary carries.
///
/// Idempotent and best-effort: a module that fails to write simply has no
/// mirrored file, and navigation to it returns `None` exactly as it did
/// before. Never fatal — a read-only or missing VARN_HOME must not stop the
/// server from serving diagnostics and completions.
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
        // Content-equal file already there: leave it, so a second session
        // costs one read per module instead of a rewrite.
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

/// Mirrored file for `specifier`, if one was written.
pub fn path_for(specifier: &str) -> Option<PathBuf> {
    let path = path_for_specifier(specifier)?;
    path.is_file().then_some(path)
}

/// Recovers the module specifier a stdlib file belongs to — the reverse of
/// [`resolve_module_file`], and like it, aware of both storage forms.
///
/// The mirror lays modules out as `<root>/<kind>/<name>.vn`, so its last two
/// components are the two halves of the specifier. An active std source tree
/// is flat (`<root>/math.vn`) and holds `std:` only. Anything else is user
/// code and yields `None`.
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

/// Whether a URI points into the mirror — i.e. a stdlib module rather than
/// user code.
pub fn is_mirrored_uri(uri: &str) -> bool {
    let path =
        varn_modules::resolver::normalize_display_path(&varn_modules::resolver::uri_to_path(uri));
    let root = varn_modules::resolver::normalize_display_path(&root().to_string_lossy());
    path.starts_with(&root)
}

/// On-disk file backing a `std:`/`core:`/`runtime:` module, canonicalized.
///
/// The active std tree wins when there is one: in a checkout, navigation
/// should land on the file you can actually edit, not on a read-only copy of
/// it. Otherwise the mirror serves it.
pub fn resolve_module_file(specifier: &str) -> Option<PathBuf> {
    let provider = varn_modules::provider::get()?;
    let path = provider
        .source_path(specifier)
        .filter(|p| p.is_file())
        .or_else(|| path_for(specifier))?;
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}
