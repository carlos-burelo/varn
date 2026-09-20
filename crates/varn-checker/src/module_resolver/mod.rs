pub mod cache;
pub mod exports;
pub mod graph;
pub mod paths;
pub mod resolver;

pub use cache::{deserialize_module_interface, serialize_module_interface, ExportMap};
pub use graph::ModuleGraph;
pub use paths::{is_known_module, resolve_package_specifier_path};
pub use resolver::{DiskResolver, ImportResolver};

/// Which carrier a module's text came from. Participates in the interface
/// cache key because the carrier can shape the bind (see ADR-0011).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CarrierKind {
    Memory = 0,
    File = 1,
    Bundle = 2,
    Native = 3,
}

impl From<varn_modules::loader::Provenance> for CarrierKind {
    fn from(p: varn_modules::loader::Provenance) -> Self {
        match p {
            varn_modules::loader::Provenance::Memory => CarrierKind::Memory,
            varn_modules::loader::Provenance::File(_) => CarrierKind::File,
            varn_modules::loader::Provenance::Bundle => CarrierKind::Bundle,
            varn_modules::loader::Provenance::Native => CarrierKind::Native,
        }
    }
}
