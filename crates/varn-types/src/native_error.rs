use crate::VmValue;
use varn_core::{ErrorField, RuntimeErrorKind, StdErrorClass};

#[derive(Clone, Debug)]
pub enum ErrorValue {
    Str(String),
    Int(i64),
}

#[derive(Clone, Debug)]
pub struct StdErrorInfo {
    pub class: StdErrorClass,
    pub fields: Vec<(ErrorField, ErrorValue)>,
}

#[derive(Clone)]
pub struct NativeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
    pub thrown: Option<VmValue>,
    pub std: Option<Box<StdErrorInfo>>,
}

impl NativeError {
    fn of_kind(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            thrown: None,
            std: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::Error, message)
    }

    pub fn std(class: StdErrorClass, message: impl Into<String>) -> Self {
        Self {
            std: Some(Box::new(StdErrorInfo {
                class,
                fields: Vec::new(),
            })),
            ..Self::of_kind(RuntimeErrorKind::Error, message)
        }
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::TypeError, message)
    }

    pub fn range_error(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::RangeError, message)
    }

    pub fn capability(capability: &str, target: impl Into<String>) -> Self {
        let target = target.into();
        Self::std(
            StdErrorClass::Capability,
            format!("permission denied ({capability}) for {target}"),
        )
        .with_str(ErrorField::Capability, capability)
        .with_str(ErrorField::Target, target)
    }

    pub fn with_str(mut self, field: ErrorField, value: impl Into<String>) -> Self {
        if let Some(info) = self.std.as_mut() {
            info.fields.push((field, ErrorValue::Str(value.into())));
        }
        self
    }

    pub fn with_int(mut self, field: ErrorField, value: i64) -> Self {
        if let Some(info) = self.std.as_mut() {
            info.fields.push((field, ErrorValue::Int(value)));
        }
        self
    }

    pub fn integer_overflow(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::IntegerOverflow, message)
    }

    pub fn division_by_zero(message: impl Into<String>) -> Self {
        Self::of_kind(RuntimeErrorKind::DivisionByZero, message)
    }

    pub fn rethrow(
        kind: RuntimeErrorKind,
        message: impl Into<String>,
        thrown: Option<VmValue>,
    ) -> Self {
        Self {
            thrown,
            ..Self::of_kind(kind, message)
        }
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
            .field("class", &self.std.as_ref().map(|info| info.class))
            .field("message", &self.message)
            .field("thrown", &self.thrown.is_some())
            .finish()
    }
}
