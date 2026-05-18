use std::fmt;

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub fn_name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub frames: Vec<FrameInfo>,

    pub thrown: Option<crate::value::VmValue>,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            frames: Vec::new(),
            thrown: None,
        }
    }

    pub fn with_frames(message: impl Into<String>, frames: Vec<FrameInfo>) -> Self {
        Self {
            message: message.into(),
            frames,
            thrown: None,
        }
    }

    pub fn with_thrown(message: impl Into<String>, thrown: crate::value::VmValue) -> Self {
        Self {
            message: message.into(),
            frames: Vec::new(),
            thrown: Some(thrown),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
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
