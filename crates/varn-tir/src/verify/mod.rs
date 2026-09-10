//! The verifier.
//!
//! Runs on every compilation, like `ssa/verify.rs` does today — not behind
//! cfg(debug_assertions). It is the only instrument that works while the
//! corpus does not run: `vn debug -p bytecode` compiles without executing, so
//! verifying the 191 modules of the corpus catches incoherence over real code
//! with nothing running.

mod coherence;
mod wellformed;

use crate::node::{Span, TirModule};

#[derive(Debug, Clone, PartialEq)]
pub struct VerifyError {
    pub message: String,
    pub span: Span,
}

impl VerifyError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        VerifyError {
            message: message.into(),
            span,
        }
    }
}

/// Verify a module. Returns every error found, not just the first: a single
/// missing case in the emitter usually produces many, and seeing them together
/// is what identifies the case.
pub fn verify_module(m: &TirModule) -> Result<(), Vec<VerifyError>> {
    let mut errors = Vec::new();
    wellformed::check(m, &mut errors);
    coherence::check(m, &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
