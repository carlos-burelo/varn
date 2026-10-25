use crate::exit::ExitCode;
use std::fmt;
use varn_pipeline::Failure;
use varn_project::ManifestError;

pub enum CliError {
    Failure(Failure),
    Fatal(String),
    Usage(String),
    Summary { message: String, exit: ExitCode },
}

impl CliError {
    pub fn fatal(message: impl Into<String>) -> Self {
        CliError::Fatal(message.into())
    }

    pub fn usage(message: impl Into<String>) -> Self {
        CliError::Usage(message.into())
    }

    pub fn exit_code(&self) -> ExitCode {
        match self {
            CliError::Failure(Failure::Diagnostics(_)) => ExitCode::Diagnostics,
            CliError::Failure(
                Failure::Runtime(_) | Failure::Io(_) | Failure::Internal(_) | Failure::Project(_),
            )
            | CliError::Fatal(_) => ExitCode::Fatal,
            CliError::Usage(_) => ExitCode::Usage,
            CliError::Summary { exit, .. } => *exit,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Failure(failure) => write!(f, "{failure}"),
            CliError::Fatal(message)
            | CliError::Usage(message)
            | CliError::Summary { message, .. } => write!(f, "{message}"),
        }
    }
}

impl From<Failure> for CliError {
    fn from(failure: Failure) -> Self {
        CliError::Failure(failure)
    }
}

impl From<ManifestError> for CliError {
    fn from(error: ManifestError) -> Self {
        CliError::Fatal(error.to_string())
    }
}
