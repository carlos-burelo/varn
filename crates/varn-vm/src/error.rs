use std::fmt;

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub class: String,
    pub message: String,
    pub frames: Vec<FrameInfo>,
    pub cause: Option<Box<RuntimeError>>,

    pub thrown: Option<crate::value::VmValue>,
    pub kind: varn_core::RuntimeErrorKind,
    pub std: Option<Box<varn_types::StdErrorInfo>>,
}

impl RuntimeError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::Error, message)
    }

    pub(crate) fn integer_overflow(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::IntegerOverflow, message)
    }

    pub(crate) fn division_by_zero(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::DivisionByZero, message)
    }

    pub(crate) fn range_error(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::RangeError, message)
    }

    pub(crate) fn type_error(message: impl Into<String>) -> Self {
        Self::of_kind(varn_core::RuntimeErrorKind::TypeError, message)
    }

    pub(crate) fn out_of_range(index: i64, len: usize) -> Self {
        Self::of_kind(
            varn_core::RuntimeErrorKind::RangeError,
            format!("index {index} is out of range for length {len}"),
        )
    }

    fn of_kind(kind: varn_core::RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self {
            class: kind.class_name().to_owned(),
            message: message.into(),
            frames: Vec::new(),
            cause: None,
            thrown: None,
            kind,
            std: None,
        }
    }
}

impl From<varn_types::NativeError> for RuntimeError {
    fn from(e: varn_types::NativeError) -> Self {
        let class = e
            .std
            .as_ref()
            .map_or_else(|| e.kind.class_name(), |info| info.class.name())
            .to_owned();
        Self {
            class,
            thrown: e.thrown,
            std: e.std,
            ..Self::of_kind(e.kind, e.message)
        }
    }
}

impl From<RuntimeError> for varn_types::NativeError {
    fn from(e: RuntimeError) -> Self {
        varn_types::NativeError {
            std: e.std,
            ..varn_types::NativeError::rethrow(e.kind, e.message, e.thrown)
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class, self.message)?;
        for frame in &self.frames {
            write!(
                f,
                "\n    at {} ({}:{})",
                frame.fn_name, frame.file, frame.line
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for RuntimeError {}

pub type VmResult<T> = Result<T, RuntimeError>;
