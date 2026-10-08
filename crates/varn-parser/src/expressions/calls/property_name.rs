use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind};

pub(super) fn parse_property_name(s: &mut TokenStream) -> ExprId {
    if s.is_eof() || s.line() > s.prev_line() {
        let range = s.prev_end_range();
        s.push_error("expected a property name after `.`".to_owned(), range);
        return s.expr(range, ExprKind::Missing);
    }
    let name = s.consume_lexeme();
    let range = s.prev_range();
    s.expr(range, ExprKind::Identifier { name })
}
