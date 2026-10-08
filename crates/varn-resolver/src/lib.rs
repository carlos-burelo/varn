pub mod cache;
pub mod exports;
pub mod graph;
mod resolver_access;
mod resolver_disk;
mod resolver_embed;
mod resolver_parse;
mod resolver_trait;

pub use graph::ModuleGraph;
pub use resolver_disk::DiskResolver;

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
