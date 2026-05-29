use std::fmt;

pub struct PipelineError {
    pub message: String,
    pub exit_code: i32,
}

impl PipelineError {
    pub fn new(exit_code: i32, message: impl Into<String>) -> Self {
        PipelineError {
            message: message.into(),
            exit_code,
        }
    }
    pub fn fatal(message: impl Into<String>) -> Self {
        PipelineError::new(1, message)
    }
    pub fn usage(message: impl Into<String>) -> Self {
        PipelineError::new(2, message)
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<String> for PipelineError {
    fn from(s: String) -> Self {
        PipelineError::fatal(s)
    }
}

impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        PipelineError::fatal(e.to_string())
    }
}
