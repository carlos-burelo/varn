use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

pub(super) fn parse_function_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    s.advance();
    parse_function_expr_inner_with_start(s, false, start_range)
}

pub(super) fn parse_function_expr_inner_with_start(
    s: &mut TokenStream,
    is_async: bool,
    start_range: varn_core::SourceRange,
) -> Result<ExprId, String> {
    let is_generator = s.eat(TokenKind::Star);
    let id = if s.check(TokenKind::Identifier) {
        Some(s.consume_lexeme())
    } else {
        None
    };
    let params = crate::parser::parse_params(s)?;
    let return_type = if s.eat(TokenKind::Colon) {
        Some(crate::types::parse_type(s)?)
    } else {
        None
    };
    let body = crate::parser::parse_block(s)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(
        full_range,
        ExprKind::Function {
            fn_id: id,
            params,
            return_type,
            body,
            is_async,
            is_generator,
        },
    ))
}
