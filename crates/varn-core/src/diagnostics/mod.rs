mod bag;
mod catalog;
mod diagnostic;
mod report;
mod suggestion;

pub use bag::DiagnosticBag;
pub use catalog::ErrorCode;
pub use diagnostic::{Diagnostic, DiagnosticKind, RelatedInformation};
pub use report::format_diagnostic;
pub use crate::source::{SourceLocation, SourceRange};
pub use suggestion::Suggestion;
