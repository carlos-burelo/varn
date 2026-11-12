use super::super::parse_assign_expr;
use crate::error::{PResult, ParseError};
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use vn_core::ast::expr::Arg;
use vn_core::ast::{ExprId, ExprKind};
use vn_core::SourceRange;
use vn_core::TokenKind;

pub(super) fn try_parse_generic_call(
    s: &mut TokenStream,
    callee: ExprId,
    expr_range: SourceRange,
) -> PResult<ExprId> {
    let type_args = parse_type_args(s)?;
    if !s.check(TokenKind::LParen) {
        return Err(ParseError::invalid("not a generic call".to_owned()));
    }
    let (_, args, call_range) = parse_call_args(s)?;
    Ok(s.expr(
        expr_range.to(call_range),
        ExprKind::Call {
            callee,
            type_args,
            args,
            optional: false,
        },
    ))
}

pub fn parse_call_args(
    s: &mut TokenStream,
) -> PResult<(Vec<vn_core::ast::TypeNode>, Vec<Arg>, SourceRange)> {
    s.expect(TokenKind::LParen)?;
    let mut args = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            args.push(Arg::Spread(parse_assign_expr(s)?));
        } else if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Colon {
            let label = s.consume_lexeme();
            s.advance();
            let label = s.interner.resolve(label).to_string();
            args.push(Arg::Named {
                label,
                value: parse_assign_expr(s)?,
            });
        } else {
            args.push(Arg::Positional(parse_assign_expr(s)?));
        }
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    let rparen = s.expect_token(TokenKind::RParen)?;
    Ok((vec![], args, rparen.range))
}

pub fn parse_call_args_pub(
    s: &mut TokenStream,
) -> PResult<(Vec<vn_core::ast::TypeNode>, Vec<Arg>, SourceRange)> {
    parse_call_args(s)
}
