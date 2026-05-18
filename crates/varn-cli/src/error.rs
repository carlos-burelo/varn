use std::fmt;

pub struct CliError {
    pub message: String,
    pub exit_code: i32,
}

impl CliError {
    pub fn new(exit_code: i32, message: impl Into<String>) -> Self {
        CliError {
            message: message.into(),
            exit_code,
        }
    }
    pub fn fatal(message: impl Into<String>) -> Self {
        CliError::new(1, message)
    }
    pub fn usage(message: impl Into<String>) -> Self {
        CliError::new(2, message)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<String> for CliError {
    fn from(s: String) -> Self {
        CliError::fatal(s)
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::fatal(e.to_string())
    }
}
