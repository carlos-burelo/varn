use std::fmt;

#[derive(Debug)]
pub struct CliError {
    pub exit_code: i32,
    pub message: String,
}

impl CliError {
    pub fn new(exit_code: i32, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(1, message)
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(1, message)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}
