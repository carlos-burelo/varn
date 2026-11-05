use std::fmt;
use std::sync::Arc;
use varn_core::term::chalk::chalk;
use varn_core::DiagnosticBag;

pub struct DiagnosticReport {
    pub file: Arc<str>,
    pub source: Arc<str>,
    pub diagnostics: DiagnosticBag,
}

impl DiagnosticReport {
    pub fn new(file: &str, source: &str, mut diagnostics: DiagnosticBag) -> Self {
        diagnostics.normalize();
        let items = diagnostics.take();
        for mut diagnostic in items {
            diagnostic.file = Arc::from(relative_path(&diagnostic.file));
            for related in &mut diagnostic.related {
                related.file = Arc::from(relative_path(&related.file));
            }
            diagnostics.push(diagnostic);
        }
        Self {
            file: Arc::from(relative_path(file)),
            source: Arc::from(source),
            diagnostics,
        }
    }
}

pub struct RuntimeFailure {
    pub class: String,
    pub message: String,
    pub frames: Vec<varn_vm::FrameInfo>,
    pub cause: Option<Box<RuntimeFailure>>,
}

impl RuntimeFailure {
    pub fn of(error: varn_vm::RuntimeError) -> Self {
        Self {
            class: error.class,
            message: error.message,
            frames: error.frames,
            cause: error.cause.map(|inner| Box::new(Self::of(*inner))),
        }
    }
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
        Failure::Diagnostics(DiagnosticReport::new(file, source, diagnostics))
    }
}

pub fn relative_path(file: &str) -> String {
    let cleaned = varn_core::diagnostics::display_path(file);
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            let root = varn_core::diagnostics::display_path(&cwd.to_string_lossy());
            cleaned.strip_prefix(&format!("{root}/")).map(str::to_owned)
        })
        .unwrap_or(cleaned)
}

const FRAMES_SHOWN_FIRST: usize = 12;
const FRAMES_SHOWN_LAST: usize = 6;

impl RuntimeFailure {
    fn write_chain(&self, f: &mut fmt::Formatter<'_>, label: &str) -> fmt::Result {
        let class = if self.class == varn_core::RuntimeErrorKind::Error.class_name() {
            String::new()
        } else {
            format!("{}: ", self.class)
        };
        write!(
            f,
            "{}{}",
            chalk(label).red().bold(),
            chalk(format!(": {class}{}", self.message)).bold()
        )?;
        let omitted = self
            .frames
            .len()
            .saturating_sub(FRAMES_SHOWN_FIRST + FRAMES_SHOWN_LAST);
        for (at, frame) in self.frames.iter().enumerate() {
            if omitted > 0 && at >= FRAMES_SHOWN_FIRST && at < FRAMES_SHOWN_FIRST + omitted {
                if at == FRAMES_SHOWN_FIRST {
                    write!(
                        f,
                        "\n    {}",
                        chalk(format!("... {omitted} more frames")).dim()
                    )?;
                }
                continue;
            }
            write!(
                f,
                "\n    {} {} {}",
                chalk("at").dim(),
                frame.fn_name,
                chalk(format!("({}:{})", relative_path(&frame.file), frame.line)).dim()
            )?;
        }
        if let Some(cause) = &self.cause {
            writeln!(f)?;
            cause.write_chain(f, "caused by")?;
        }
        Ok(())
    }
}

impl fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_chain(f, "error")
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
