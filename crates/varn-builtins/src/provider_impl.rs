use std::path::PathBuf;
use std::sync::OnceLock;

use varn_modules::bundle::{read_bundle, StdBundle};
use varn_modules::provider::StdlibProvider;
use varn_modules::spec::{ModuleKind, ModuleSpec};
use varn_modules::std_root::{resolve, StdProvenance, StdSource};

use crate::loader::CoreSourceLocator;
use crate::registry::{spec_for, MODULE_REGISTRY};

/// Active std resolved once per process. Specs + blobs are leaked to satisfy
/// the provider's &'static contract; the std set is small and process-lived.
struct ActiveStd {
    specs: &'static [ModuleSpec],
    /// (id, interface, bytecode) — bundle mode only.
    blobs: &'static [(String, &'static [u8], &'static [u8])],
    /// Source tree root — tree mode only.
    tree_root: Option<PathBuf>,
    description: String,
    provenance: StdProvenance,
}

static ACTIVE_STD: OnceLock<Option<ActiveStd>> = OnceLock::new();
static EMBEDDED_STDLIB_BYTES: OnceLock<&'static [u8]> = OnceLock::new();

pub fn register_embedded_stdlib(bytes: &'static [u8]) {
    let _ = EMBEDDED_STDLIB_BYTES.set(bytes);
}

fn active_std() -> Option<&'static ActiveStd> {
    ACTIVE_STD
        .get_or_init(|| {
            if let Some((source, provenance)) = resolve() {
                match source {
                    StdSource::Bundle(path) => load_bundle_std(&path, provenance),
                    StdSource::SourceTree(root) => load_tree_std(&root, provenance),
                }
            } else if let Some(bytes) = EMBEDDED_STDLIB_BYTES.get() {
                load_embedded_std(bytes)
            } else {
                None
            }
        })
        .as_ref()
}

fn load_embedded_std(bytes: &'static [u8]) -> Option<ActiveStd> {
    let bundle: StdBundle = match read_bundle(bytes) {
        Ok(b) => b,
        Err(e) => {
            panic!("embedded stdlib corrupt: {e}");
        }
    };
    if let Err(e) = bundle.validate_compat_with(varn_core::HOST_API_VERSION) {
        panic!("embedded stdlib incompatible: {e}");
    }
    let mut specs = Vec::new();
    let mut blobs = Vec::new();
    for m in bundle.modules {
        specs.push(ModuleSpec::leaked(
            m.id.clone(),
            ModuleKind::Stdlib,
            String::new(),
            m.pure,
        ));
        blobs.push((
            m.id,
            &*Box::leak(m.interface.into_boxed_slice()),
            &*Box::leak(m.bytecode.into_boxed_slice()),
        ));
    }
    Some(ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        blobs: Box::leak(blobs.into_boxed_slice()),
        tree_root: None,
        description: format!("embedded stdlib v{}", bundle.std_version),
        provenance: StdProvenance::Toolchain,
    })
}

fn load_bundle_std(path: &std::path::Path, provenance: StdProvenance) -> Option<ActiveStd> {
    let bytes = std::fs::read(path)
        .map_err(|e| eprintln!("warning: cannot read std bundle {}: {e}", path.display()))
        .ok()?;
    let bundle: StdBundle = match read_bundle(&bytes) {
        Ok(b) => b,
        Err(e) => {
            // Hard error: a present-but-invalid bundle must not silently
            // fall back to the embedded registry (spec §3).
            panic!("{e} ({})", path.display());
        }
    };
    if let Err(e) = bundle.validate_compat_with(varn_core::HOST_API_VERSION) {
        panic!("{e} ({})", path.display());
    }
    let mut specs = Vec::new();
    let mut blobs = Vec::new();
    for m in bundle.modules {
        specs.push(ModuleSpec::leaked(
            m.id.clone(),
            ModuleKind::Stdlib,
            String::new(),
            m.pure,
        ));
        blobs.push((
            m.id,
            &*Box::leak(m.interface.into_boxed_slice()),
            &*Box::leak(m.bytecode.into_boxed_slice()),
        ));
    }
    Some(ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        blobs: Box::leak(blobs.into_boxed_slice()),
        tree_root: None,
        description: format!("bundle {} v{}", path.display(), bundle.std_version),
        provenance,
    })
}

fn load_tree_std(root: &std::path::Path, provenance: StdProvenance) -> Option<ActiveStd> {
    let manifest = crate::std_manifest::read_manifest(root)?;
    if manifest.host_api != varn_core::HOST_API_VERSION {
        // Hard error: mirrors the bundle-mode gate (spec §3) — a
        // present-but-incompatible source tree must not silently fall back
        // to the embedded registry.
        panic!(
            "std source tree {} requires host API v{} but this vn provides v{}",
            root.display(),
            manifest.host_api,
            varn_core::HOST_API_VERSION
        );
    }
    let mut specs = Vec::new();
    for m in manifest.modules {
        // std:math → math.vn
        let file = m.id.strip_prefix("std:").unwrap_or(&m.id);
        specs.push(ModuleSpec::leaked(
            m.id.clone(),
            ModuleKind::Stdlib,
            format!("{file}.vn"),
            m.pure,
        ));
    }
    Some(ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        blobs: &[],
        tree_root: Some(root.to_path_buf()),
        description: format!("source tree {} v{}", root.display(), manifest.version),
        provenance,
    })
}

struct BuiltinsProvider;

fn std_spec(specifier: &str) -> Option<&'static ModuleSpec> {
    active_std()?.specs.iter().find(|s| s.id == specifier)
}

/// Registry specs, minus any id the active std now serves (active std wins),
/// plus the active std's specs. Lazily computed once: consumers that iterate
/// `all_specs()` (`std_module_ids`, LSP completion) must see active-std ids
/// exactly like `spec_for` does, or a module migrated out of the embedded
/// registry silently disappears from those lists.
static COMBINED_SPECS: OnceLock<&'static [ModuleSpec]> = OnceLock::new();

fn combined_specs() -> &'static [ModuleSpec] {
    COMBINED_SPECS.get_or_init(|| match active_std() {
        None => MODULE_REGISTRY,
        Some(std) => {
            let mut combined: Vec<ModuleSpec> = MODULE_REGISTRY
                .iter()
                .filter(|m| !std.specs.iter().any(|s| s.id == m.id))
                .map(|m| ModuleSpec::leaked(m.id.to_owned(), m.kind, m.vn_source.to_owned(), m.pure))
                .collect();
            combined.extend(std.specs.iter().map(|s| {
                ModuleSpec::leaked(s.id.to_owned(), s.kind, s.vn_source.to_owned(), s.pure)
            }));
            Box::leak(combined.into_boxed_slice())
        }
    })
}

impl StdlibProvider for BuiltinsProvider {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec> {
        if specifier.starts_with("std:") {
            std_spec(specifier)
        } else {
            std_spec(specifier).or_else(|| spec_for(specifier))
        }
    }

    fn embedded_source(&self, specifier: &str) -> Option<&'static str> {
        if specifier.starts_with("std:") {
            return None;
        }
        if std_spec(specifier).is_some() {
            return None; // active std serves via source_path or blobs
        }
        spec_for(specifier)?.source()
    }

    fn source_path(&self, specifier: &str) -> Option<PathBuf> {
        if let Some(spec) = std_spec(specifier) {
            let std = active_std()?;
            let root = std.tree_root.as_ref()?;
            return Some(root.join(spec.vn_source));
        }
        CoreSourceLocator::from_env().vn_source_path(specifier)
    }

    fn all_specs(&self) -> &'static [ModuleSpec] {
        combined_specs()
    }

    fn interface_blob(&self, specifier: &str) -> Option<&'static [u8]> {
        let std = active_std()?;
        std.blobs
            .iter()
            .find(|(id, _, _)| id == specifier)
            .map(|(_, i, _)| *i)
    }

    fn bytecode_blob(&self, specifier: &str) -> Option<&'static [u8]> {
        let std = active_std()?;
        std.blobs
            .iter()
            .find(|(id, _, _)| id == specifier)
            .map(|(_, _, b)| *b)
    }

    fn std_provenance(&self) -> Option<(String, StdProvenance)> {
        let std = active_std()?;
        Some((std.description.clone(), std.provenance))
    }
}

static PROVIDER: BuiltinsProvider = BuiltinsProvider;

pub fn register_provider() {
    let total = crate::modules::force_link_builtins();
    std::hint::black_box(total);
    varn_modules::provider::register(&PROVIDER);
}

