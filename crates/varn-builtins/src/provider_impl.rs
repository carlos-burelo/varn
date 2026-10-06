use std::path::PathBuf;
use std::sync::OnceLock;

use varn_modules::bundle::{read_bundle, StdBundle};
use varn_modules::provider::StdlibProvider;
use varn_modules::spec::{ModuleKind, ModuleSpec};
use varn_modules::std_root::{resolve, StdProvenance, StdSource};

use crate::loader::CoreSourceLocator;
use crate::registry::{spec_for, MODULE_REGISTRY};



struct ActiveStd {
    specs: &'static [ModuleSpec],
    
    blobs: &'static [StdModuleBlobs],
    
    tree_root: Option<PathBuf>,
    description: String,
    provenance: StdProvenance,
}



struct StdModuleBlobs {
    id: String,
    interface: &'static [u8],
    bytecode: &'static [u8],
    source: &'static str,
}

fn leak_bundle_modules(
    modules: Vec<varn_modules::bundle::BundleModule>,
) -> (Vec<ModuleSpec>, Vec<StdModuleBlobs>) {
    let mut specs = Vec::with_capacity(modules.len());
    let mut blobs = Vec::with_capacity(modules.len());
    for m in modules {
        
        
        let file = format!("{}.vn", m.id.strip_prefix("std:").unwrap_or(&m.id));
        specs.push(ModuleSpec::leaked(
            m.id.clone(),
            ModuleKind::Stdlib,
            file,
            m.pure,
        ));
        blobs.push(StdModuleBlobs {
            id: m.id,
            interface: Box::leak(m.interface.into_boxed_slice()),
            bytecode: Box::leak(m.bytecode.into_boxed_slice()),
            source: Box::leak(m.source.into_boxed_str()),
        });
    }
    (specs, blobs)
}







static ACTIVE_STD: OnceLock<Result<Option<ActiveStd>, String>> = OnceLock::new();
static EMBEDDED_STDLIB_BYTES: OnceLock<&'static [u8]> = OnceLock::new();

pub fn register_embedded_stdlib(bytes: &'static [u8]) {
    let _ = EMBEDDED_STDLIB_BYTES.set(bytes);
}

fn active_std_result() -> &'static Result<Option<ActiveStd>, String> {
    ACTIVE_STD.get_or_init(|| {
        let (source, provenance) = resolve();
        match source {
            StdSource::SourceTree(root) => load_tree_std(&root, provenance).map(Some),
            StdSource::Embedded => match EMBEDDED_STDLIB_BYTES.get() {
                Some(bytes) => load_embedded_std(bytes).map(Some),
                
                
                
                
                
                None if provenance == StdProvenance::Env => Err(format!(
                    "{}={} was requested but this binary has no stdlib compiled into it",
                    varn_modules::std_root::ENV_VARN_STD,
                    varn_modules::std_root::STD_EMBEDDED_SENTINEL
                )),
                None => Ok(None),
            },
        }
    })
}

fn active_std() -> Option<&'static ActiveStd> {
    active_std_result().as_ref().ok()?.as_ref()
}







pub fn std_load_error() -> Option<&'static str> {
    active_std_result().as_ref().err().map(String::as_str)
}

fn load_embedded_std(bytes: &'static [u8]) -> Result<ActiveStd, String> {
    let bundle: StdBundle =
        read_bundle(bytes).map_err(|e| format!("embedded stdlib corrupt: {e}"))?;
    bundle
        .validate_compat_with(varn_core::HOST_API_VERSION)
        .map_err(|e| format!("embedded stdlib incompatible: {e}"))?;
    let (specs, blobs) = leak_bundle_modules(bundle.modules);
    Ok(ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        blobs: Box::leak(blobs.into_boxed_slice()),
        tree_root: None,
        description: format!("embedded stdlib v{}", bundle.std_version),
        provenance: StdProvenance::Embedded,
    })
}

fn load_tree_std(root: &std::path::Path, provenance: StdProvenance) -> Result<ActiveStd, String> {
    
    
    
    let manifest = crate::std_manifest::read_manifest(root)
        .ok_or_else(|| format!("cannot read std manifest in source tree {}", root.display()))?;
    if manifest.host_api != varn_core::HOST_API_VERSION {
        return Err(format!(
            "std source tree {} requires host API v{} but this vn provides v{}",
            root.display(),
            manifest.host_api,
            varn_core::HOST_API_VERSION
        ));
    }
    let mut specs = Vec::new();
    for m in manifest.modules {
        
        let file = m.id.strip_prefix("std:").unwrap_or(&m.id);
        let vn_source = if root.join(format!("{file}/mod.vn")).exists() {
            format!("{file}/mod.vn")
        } else {
            format!("{file}.vn")
        };
        specs.push(ModuleSpec::leaked(
            m.id.clone(),
            ModuleKind::Stdlib,
            vn_source,
            m.pure,
        ));
    }
    Ok(ActiveStd {
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

fn std_blobs(specifier: &str) -> Option<&'static StdModuleBlobs> {
    active_std()?.blobs.iter().find(|b| b.id == specifier)
}






static COMBINED_SPECS: OnceLock<&'static [ModuleSpec]> = OnceLock::new();

fn combined_specs() -> &'static [ModuleSpec] {
    COMBINED_SPECS.get_or_init(|| match active_std() {
        None => MODULE_REGISTRY,
        Some(std) => {
            let mut combined: Vec<ModuleSpec> = MODULE_REGISTRY
                .iter()
                .filter(|m| !std.specs.iter().any(|s| s.id == m.id))
                .copied()
                .collect();
            combined.extend(std.specs.iter().copied());
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
            return None; 
        }
        spec_for(specifier)?.source()
    }

    fn source_path(&self, specifier: &str) -> Option<PathBuf> {
        if let Some(spec) = std_spec(specifier) {
            let std = active_std()?;
            let root = std.tree_root.as_ref()?;
            return Some(root.join(spec.vn_source));
        }
        CoreSourceLocator::from_checkout().vn_source_path(specifier)
    }

    fn all_specs(&self) -> &'static [ModuleSpec] {
        combined_specs()
    }

    fn interface_blob(&self, specifier: &str) -> Option<&'static [u8]> {
        Some(std_blobs(specifier)?.interface)
    }

    fn bundled_source(&self, specifier: &str) -> Option<&'static str> {
        Some(std_blobs(specifier)?.source)
    }

    fn bytecode_blob(&self, specifier: &str) -> Option<&'static [u8]> {
        Some(std_blobs(specifier)?.bytecode)
    }

    fn std_provenance(&self) -> Option<(String, StdProvenance)> {
        let std = active_std()?;
        Some((std.description.clone(), std.provenance))
    }
}

static PROVIDER: BuiltinsProvider = BuiltinsProvider;

pub fn register_provider() {
    #[cfg(feature = "runtime")]
    {
        let total = crate::modules::force_link_builtins();
        std::hint::black_box(total);
    }
    varn_modules::provider::register(&PROVIDER);
}
