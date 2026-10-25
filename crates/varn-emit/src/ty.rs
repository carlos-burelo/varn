mod lower;
mod resolve;
mod union;

pub use lower::{lower_type, prime, NULL_TY};
pub use resolve::{InterfaceProperty, NameResolver, NoNames, ScopedTypeParams};
