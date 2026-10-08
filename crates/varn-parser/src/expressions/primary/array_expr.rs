use super::super::parse_assign_expr;
use crate::stream::TokenStream;
use varn_core::ast::{ArrayEl, ExprId, ExprKind};
use varn_core::TokenKind;

pub(super) fn parse_array_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    s.advance();
    let mut elements = vec![];

    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        if s.check(TokenKind::Comma) {
            elements.push(ArrayEl::Hole);
            s.advance();
            continue;
        }
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            elements.push(ArrayEl::Spread(parse_assign_expr(s)?));
        } else {
            elements.push(ArrayEl::Expr(parse_assign_expr(s)?));
        }
        s.eat(TokenKind::Comma);
    }

    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::Array { elements }))
}
