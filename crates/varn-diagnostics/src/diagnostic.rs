use crate::catalog::ErrorCode;
use crate::source::SourceRange;
use crate::suggestion::Suggestion;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    Error,
    Warning,
    Hint,
}

#[derive(Clone, Debug)]
pub struct RelatedInformation {
    pub message: String,
    pub file: Rc<str>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub kind: DiagnosticKind,
    pub message: String,
    pub range: SourceRange,
    pub file: Rc<str>,
    pub suggestions: Vec<Suggestion>,
    pub related: Vec<RelatedInformation>,
}

impl Diagnostic {
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            kind: DiagnosticKind::Error,
            message: message.into(),
            range: SourceRange::default(),
            file: Rc::from(""),
            suggestions: Vec::new(),
            related: Vec::new(),
        }
    }

    pub fn warning(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            kind: DiagnosticKind::Warning,
            message: message.into(),
            range: SourceRange::default(),
            file: Rc::from(""),
            suggestions: Vec::new(),
            related: Vec::new(),
        }
    }

    pub fn hint(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            kind: DiagnosticKind::Hint,
            message: message.into(),
            range: SourceRange::default(),
            file: Rc::from(""),
            suggestions: Vec::new(),
            related: Vec::new(),
        }
    }

    pub fn with_file(mut self, file: impl Into<Rc<str>>) -> Self {
        self.file = file.into();
        self
    }

    pub fn with_range(mut self, range: SourceRange) -> Self {
        self.range = range;
        self
    }

    pub fn with_location(mut self, file: &str, line: u32, col: u32) -> Self {
        self.file = Rc::from(file);
        self.range.start.line = line;
        self.range.start.column = col;
        self.range.end.line = line;
        self.range.end.column = col;
        self
    }

    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    pub fn with_related(
        mut self,
        message: impl Into<String>,
        file: impl Into<Rc<str>>,
        range: SourceRange,
    ) -> Self {
        self.related.push(RelatedInformation {
            message: message.into(),
            file: file.into(),
            range,
        });
        self
    }

    pub fn is_error(&self) -> bool {
        self.kind == DiagnosticKind::Error
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            DiagnosticKind::Error => "error",
            DiagnosticKind::Warning => "warning",
            DiagnosticKind::Hint => "hint",
        };
        write!(
            f,
            "{} {} [{}:{}] {}",
            kind, self.code, self.file, self.range.start.line, self.message
        )
    }
}
