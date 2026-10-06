mod dead_code;
mod purity;
mod trivial_phi;

pub use dead_code::run;
pub(crate) use purity::{dest_droppable, is_pure};
