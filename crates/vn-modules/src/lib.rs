pub mod artifact;
pub mod contain;
pub mod ids;
pub mod layer;
pub mod loader;
pub mod package;
pub mod provider;
pub mod registry;
pub mod resolver;
pub mod spec;
pub mod std_image;
pub mod std_root;
pub mod uri;

pub use ids::{DEFAULT_PACKAGE_VERSION, PKG_PREFIX, RELATIVE_EXPORT_PREFIX, VARN_FILE_EXTENSION};
pub use package::{
    canonical_or_original, is_pkg_specifier, normalize_path_string, resolve_pkg_specifier,
    resolve_pkg_specifier_detailed, PackageResolution,
};
pub use registry::{
    core_module_ids, is_known_stdlib_module, is_prelude_module, prelude_modules, std_module_ids,
};
pub use spec::{ModuleKind, ModuleSpec};
