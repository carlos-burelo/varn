use super::parse_assign_expr;
use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

pub(super) fn parse_yield_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    s.advance();
    let delegate = s.eat(TokenKind::Star);
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(parse_assign_expr(s)?)
    } else {
        None
    };
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::Yield { argument, delegate }))
}
