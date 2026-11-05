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

    pub fn diagnostics(file: &str, source: &str, mut diagnostics: DiagnosticBag) -> Self {
        diagnostics.normalize();
        let items = diagnostics.take();
        for mut diagnostic in items {
            diagnostic.file = Arc::from(relative_path(&diagnostic.file));
            for related in &mut diagnostic.related {
                related.file = Arc::from(relative_path(&related.file));
            }
            diagnostics.push(diagnostic);
        }
        Failure::Diagnostics(DiagnosticReport {
            file: Arc::from(relative_path(file)),
            source: Arc::from(source),
            diagnostics,
        })
    }
}

pub fn relative_path(file: &str) -> String {
    let cleaned = varn_core::diagnostics::display_path(file);
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            let root = varn_core::diagnostics::display_path(&cwd.to_string_lossy());
            cleaned
                .strip_prefix(&format!("{root}/"))
                .map(str::to_owned)
        })
        .unwrap_or(cleaned)
}

impl fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        for frame in &self.frames {
            write!(
                f,
                "\n    at {} ({}:{})",
                frame.fn_name,
                relative_path(&frame.file),
                frame.line
            )?;
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
