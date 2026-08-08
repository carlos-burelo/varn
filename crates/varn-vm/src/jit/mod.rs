//! The VM side of the JIT boundary: what gets compiled (tiering) and what
//! compiled code is allowed to call back into (helpers).

pub mod helpers;
pub mod tiering;
