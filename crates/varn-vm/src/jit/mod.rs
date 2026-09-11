//! The VM side of the JIT boundary: what gets compiled (tiering) and what
//! compiled code is allowed to call back into (helpers).

pub(crate) mod frame_layout;
pub mod helpers;
pub(crate) mod tiering;
