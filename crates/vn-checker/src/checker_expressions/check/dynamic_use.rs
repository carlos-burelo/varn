use crate::checker::Checker;
use vn_core::source::SourceRange;
use vn_core::{Diagnostic, ErrorCode};
use vn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn reject_dynamic_operand(
        &mut self,
        ty: &Type,
        action: &str,
        range: SourceRange,
    ) -> bool {
        if !ty.is_dynamic() {
            return false;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::DynamicNeedsCast,
                format!("cannot {action} a value of type 'dynamic': cast it with `as T` first"),
            )
            .with_range(range),
        );
        true
    }
}
