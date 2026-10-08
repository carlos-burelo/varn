mod arrays;
mod compat_fn_sig;
mod compat_lookup;
mod core;
mod expr_sat;
mod generics;
mod nominal;
mod objects;
mod resolve;
mod scalar;
mod unions;

pub use core::{types_compatible, types_compatible_with_cache};
pub use expr_sat::expr_satisfies_target_type;
