use super::super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use varn_core::ast::expr::Arg;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::SourceRange;
use varn_core::TokenKind;

pub(super) fn looks_like_generic_call(s: &TokenStream) -> bool {
    let mut depth = 0i32;
    let mut off = 0usize;
    loop {
        match s.peek_kind(off) {
            TokenKind::LAngle => depth += 1,
            TokenKind::RAngle => {
                depth -= 1;
                if depth == 0 {
                    return s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGt => {
                depth -= 2;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGtGt => {
                depth -= 3;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::EOF
            | TokenKind::Semicolon
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::Eq
            | TokenKind::FatArrow => return false,
            _ => {}
        }
        off += 1;
    }
}

pub(super) fn try_parse_generic_call(
    s: &mut TokenStream,
    callee: ExprId,
    expr_range: SourceRange,
) -> Result<ExprId, String> {
    let type_args = parse_type_args(s)?;
    if !s.check(TokenKind::LParen) {
        return Err("not a generic call".to_owned());
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
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
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
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
    parse_call_args(s)
}
