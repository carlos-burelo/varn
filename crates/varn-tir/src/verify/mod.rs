







mod coherence;
mod wellformed;

use crate::node::Span;
use crate::TirModule;

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
