//! `VARN_HOME_TRACE`: per-call tracing of home-slot and native-call traffic,
//! for debugging the compiled/interpreted frame boundary.
//!
//! Read once. The helpers that print under it run on every native call,
//! field access and method call; asking the environment each time made
//! `getenv` the single largest cost of a string-heavy program (41% of its
//! instructions).

use std::sync::OnceLock;

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VARN_HOME_TRACE").is_some())
}
