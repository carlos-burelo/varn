mod arrays;
mod compat_lookup;
mod core;
mod expr_sat;
mod generics;
mod nominal;
mod objects;
mod resolve;
mod scalar;
mod unions;

pub(crate) use core::{types_compatible, types_compatible_with_cache};
pub(crate) use expr_sat::expr_satisfies_target_type;
