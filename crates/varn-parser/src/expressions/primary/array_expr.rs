use super::super::parse_assign_expr;
use crate::stream::TokenStream;
use varn_core::ast::{ArrayEl, ExprId, ExprKind, MapEntry};
use varn_core::TokenKind;

pub(super) fn parse_array_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    s.advance();
    if s.check(TokenKind::Colon) && s.peek_kind(1) == TokenKind::RBracket {
        s.advance();
        s.advance();
        let full_range = s.span_from(start_range);
        return Ok(s.expr(full_range, ExprKind::MapLiteral { entries: vec![] }));
    }
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
            let first = parse_assign_expr(s)?;
            if elements.is_empty() && s.check(TokenKind::Colon) {
                return parse_map_entries(s, start_range, first);
            }
            elements.push(ArrayEl::Expr(first));
        }
        s.eat(TokenKind::Comma);
    }

    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::Array { elements }))
}

fn parse_map_entries(
    s: &mut TokenStream,
    start_range: varn_core::SourceRange,
    first_key: ExprId,
) -> Result<ExprId, String> {
    let mut entries = vec![];
    let mut key = first_key;
    loop {
        s.expect(TokenKind::Colon)?;
        let value = parse_assign_expr(s)?;
        entries.push(MapEntry { key, value });
        if !s.eat(TokenKind::Comma) || s.check(TokenKind::RBracket) {
            break;
        }
        key = parse_assign_expr(s)?;
    }
    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::MapLiteral { entries }))
}
