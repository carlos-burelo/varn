//! Every binder diagnostic names the file it belongs to, so a report never
//! renders without a location.

use varn_core::Diagnostic;

impl super::Binder<'_> {
    pub(super) fn emit(&mut self, diag: Diagnostic) {
        let diag = if diag.file.is_empty() {
            diag.with_file(self.source_file.clone())
        } else {
            diag
        };
        self.diagnostics.push(diag);
    }
}
