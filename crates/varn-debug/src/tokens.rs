//! Re-export of the migrated `tokens` phase (kept so `varn_debug::tokens`
//! callers need no change; DEBUG_PLAN moves phases under `phases/`).

pub use crate::phases::tokens::debug_tokens;
