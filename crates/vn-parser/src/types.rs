mod combinators;
mod composite;
mod entry;
mod generics;
mod literals;
mod primary;
mod suffixes;

pub use entry::parse_type;
pub use generics::{parse_type_args, parse_type_params};
