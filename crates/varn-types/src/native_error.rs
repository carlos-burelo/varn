//! Error of a native function: carries its platform error class to the
//! `catch`, the way a VM-born `RuntimeError` does.

use varn_core::RuntimeErrorKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
}

impl NativeError {
    pub fn integer_overflow(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::IntegerOverflow,
            message: message.into(),
        }
    }

    pub fn division_by_zero(message: impl Into<String>) -> Self {
        Self {
            kind: RuntimeErrorKind::DivisionByZero,
            message: message.into(),
        }
    }
}

impl From<String> for NativeError {
    fn from(message: String) -> Self {
        Self {
            kind: RuntimeErrorKind::Error,
            message,
        }
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
