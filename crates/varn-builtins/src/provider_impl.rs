use std::path::PathBuf;
use std::sync::OnceLock;

use varn_modules::provider::StdlibProvider;
use varn_modules::spec::ModuleSpec;
use varn_modules::std_image::StdImage;
use varn_modules::std_root::StdProvenance;

use crate::loader::CoreSourceLocator;
use crate::registry::{spec_for, MODULE_REGISTRY};

pub enum StdInput {
    Image {
        bytes: &'static [u8],
        tree_root: Option<PathBuf>,
        provenance: StdProvenance,
    },
    Tree {
        root: PathBuf,
        provenance: StdProvenance,
    },
    Unavailable(String),
}

struct ActiveStd {
    specs: &'static [ModuleSpec],

    image: Option<StdImage>,

    tree_root: Option<PathBuf>,
    description: String,
    provenance: StdProvenance,
}

static STD_INPUT: OnceLock<StdInput> = OnceLock::new();
static ACTIVE_STD: OnceLock<Result<Option<ActiveStd>, String>> = OnceLock::new();

pub fn register_std(input: StdInput) {
    let _ = STD_INPUT.set(input);
}

fn active_std_result() -> &'static Result<Option<ActiveStd>, String> {
    ACTIVE_STD.get_or_init(|| match STD_INPUT.get() {
        None => Ok(None),
        Some(StdInput::Image {
            bytes,
            tree_root,
            provenance,
        }) => load_image(bytes, tree_root.clone(), *provenance).map(Some),
        Some(StdInput::Tree { root, provenance }) => Ok(Some(load_tree(root, *provenance))),
        Some(StdInput::Unavailable(reason)) => Err(reason.clone()),
    })
}

fn active_std() -> Option<&'static ActiveStd> {
    active_std_result().as_ref().ok()?.as_ref()
}

pub fn std_load_error() -> Option<&'static str> {
    active_std_result().as_ref().err().map(String::as_str)
}

fn load_image(
    bytes: &'static [u8],
    tree_root: Option<PathBuf>,
    provenance: StdProvenance,
) -> Result<ActiveStd, String> {
    let image = StdImage::parse(bytes).map_err(|e| format!("stdlib image corrupt: {e}"))?;
    if image.host_api_version != varn_core::HOST_API_VERSION {
        return Err(format!(
            "stdlib image incompatible: it requires host API v{} but this vn provides v{}",
            image.host_api_version,
            varn_core::HOST_API_VERSION
        ));
    }
    let specs: Vec<ModuleSpec> = image
        .modules()
        .iter()
        .filter(|m| m.id.starts_with(varn_modules::spec::STD_PREFIX))
        .map(|m| ModuleSpec::stdlib(m.id, m.path, m.pure))
        .collect();
    Ok(ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        description: format!("stdlib image v{}", image.std_version),
        image: Some(image),
        tree_root,
        provenance,
    })
}

fn load_tree(root: &std::path::Path, provenance: StdProvenance) -> ActiveStd {
    let specs: Vec<ModuleSpec> = varn_modules::std_root::tree_modules(root)
        .into_iter()
        .map(|m| {
            ModuleSpec::stdlib(
                Box::leak(m.id.into_boxed_str()),
                Box::leak(m.path.into_boxed_str()),
                false,
            )
        })
        .collect();
    ActiveStd {
        specs: Box::leak(specs.into_boxed_slice()),
        image: None,
        tree_root: Some(root.to_path_buf()),
        description: format!("source tree {}", root.display()),
        provenance,
    }
}

struct BuiltinsProvider;

fn std_spec(specifier: &str) -> Option<&'static ModuleSpec> {
    active_std()?.specs.iter().find(|s| s.id == specifier)
}

fn std_module(specifier: &str) -> Option<&'static varn_modules::std_image::ImageModule<'static>> {
    active_std()?.image.as_ref()?.module(specifier)
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
        Some(std_module(specifier)?.interface).filter(|b| !b.is_empty())
    }

    fn bundled_source(&self, specifier: &str) -> Option<&'static str> {
        Some(std_module(specifier)?.source)
    }

    fn bytecode_blob(&self, specifier: &str) -> Option<&'static [u8]> {
        Some(std_module(specifier)?.bytecode)
    }

    fn std_provenance(&self) -> Option<(String, StdProvenance)> {
        let std = active_std()?;
        Some((std.description.clone(), std.provenance))
    }

    fn native_op_id(&self, module: &str, namespace: &str, symbol: &str) -> Option<u64> {
        #[cfg(feature = "runtime")]
        {
            crate::dispatch::native_op_id(module, namespace, symbol)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (module, namespace, symbol);
            None
        }
    }

    fn image_identity(&self) -> u64 {
        active_std()
            .and_then(|std| std.image.as_ref())
            .map_or(0, |image| image.identity)
    }
}

static PROVIDER: BuiltinsProvider = BuiltinsProvider;

pub fn register_provider() {
    varn_modules::provider::register(&PROVIDER);
}
