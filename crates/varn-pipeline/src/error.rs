use std::fmt;
use std::sync::Arc;
use varn_core::DiagnosticBag;

pub struct DiagnosticReport {
    pub file: Arc<str>,
    pub source: Arc<str>,
    pub diagnostics: DiagnosticBag,
}

pub struct RuntimeFailure {
    pub message: String,
    pub frames: Vec<varn_vm::FrameInfo>,
}

pub enum Failure {
    Diagnostics(DiagnosticReport),
    Runtime(RuntimeFailure),
    Io(String),
    Internal(String),
    Project(varn_project::ManifestError),
}

impl Failure {
    pub fn io(message: impl Into<String>) -> Self {
        Failure::Io(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Failure::Internal(message.into())
    }

    pub fn diagnostics(file: &str, source: &str, diagnostics: DiagnosticBag) -> Self {
        Failure::Diagnostics(DiagnosticReport {
            file: Arc::from(file),
            source: Arc::from(source),
            diagnostics,
        })
    }
}

impl fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        let cwd = std::env::current_dir().ok();
        for frame in &self.frames {
            let mut file = frame.file.as_str();
            if let Some(stripped) = file.strip_prefix(r"\\?\") {
                file = stripped;
            }
            let relative = cwd
                .as_deref()
                .and_then(|root| std::path::Path::new(file).strip_prefix(root).ok())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| file.replace('\\', "/"));
            write!(f, "\n    at {} ({}:{})", frame.fn_name, relative, frame.line)?;
        }
        Ok(())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Diagnostics(report) => write!(
                f,
                "{}",
                varn_core::diagnostics::render_report(
                    &report.file,
                    &report.source,
                    &report.diagnostics
                )
            ),
            Failure::Runtime(failure) => write!(f, "{failure}"),
            Failure::Io(message) | Failure::Internal(message) => write!(f, "{message}"),
            Failure::Project(error) => write!(f, "{error}"),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::internal(message)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::io(error.to_string())
    }
}
