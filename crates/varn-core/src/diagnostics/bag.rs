use super::catalog::ErrorCode;
use super::diagnostic::{Diagnostic, DiagnosticKind};
use crate::source::SourceRange;

#[derive(Default, Debug, Clone)]
pub struct DiagnosticBag {
    items: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn emit(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn error(&mut self, code: ErrorCode, message: impl Into<String>, range: SourceRange) {
        self.items
            .push(Diagnostic::error(code, message).with_range(range));
    }

    pub fn warning(&mut self, code: ErrorCode, message: impl Into<String>, range: SourceRange) {
        self.items
            .push(Diagnostic::warning(code, message).with_range(range));
    }

    pub fn hint(&mut self, code: ErrorCode, message: impl Into<String>, range: SourceRange) {
        self.items
            .push(Diagnostic::hint(code, message).with_range(range));
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.kind == DiagnosticKind::Error)
    }

    pub fn all(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.kind == DiagnosticKind::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn extend(&mut self, other: DiagnosticBag) {
        self.items.extend(other.items);
    }

    pub fn take(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.items)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a DiagnosticBag {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for DiagnosticBag {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl std::ops::Index<usize> for DiagnosticBag {
    type Output = Diagnostic;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}
