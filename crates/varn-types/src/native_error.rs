//! Error of a native function: carries its platform error class to the
//! `catch`, the way a VM-born `RuntimeError` does — and, when a callback the
//! native ran threw, the thrown value itself, so the exception reaches the
//! caller's `catch` unchanged instead of being swallowed or flattened to text.

use crate::VmValue;
use varn_core::RuntimeErrorKind;

#[derive(Clone)]
pub struct NativeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
    pub thrown: Option<VmValue>,
}

impl NativeError {
    fn of_kind(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            thrown: None,
        }
    }

    pub fn integer_overflow(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::IntegerOverflow, message)
    }

    pub fn division_by_zero(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::DivisionByZero, message)
    }

    /// An exception a VM callback threw, rethrown as is.
    pub fn rethrow(
        kind: RuntimeErrorKind,
        message: impl Into<String>,
        thrown: Option<VmValue>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            thrown,
        }
    }
}

impl From<String> for NativeError {
    fn from(message: String) -> Self {
        Self::of_kind(RuntimeErrorKind::Error, message)
    }
}

impl From<&str> for NativeError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::fmt::Debug for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("thrown", &self.thrown.is_some())
            .finish()
    }
}
