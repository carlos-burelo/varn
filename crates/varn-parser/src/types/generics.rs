use crate::stream::TokenStream;
use varn_core::ast::TypeNode;
use varn_core::TokenKind;

pub fn parse_type_args(s: &mut TokenStream) -> Result<Vec<TypeNode>, String> {
    s.expect(TokenKind::LAngle)?;
    let mut args = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        args.push(super::entry::parse_type(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect_rangle()?;
    Ok(args)
}

pub fn parse_type_params(
    s: &mut TokenStream,
) -> Result<Vec<varn_core::ast::TypeParam>, String> {
    s.expect(TokenKind::LAngle)?;
    let mut params = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        let range = s.range();
        let name = s.expect_id()?;
        let constraint = if s.eat(TokenKind::Extends) {
            Some(super::combinators::parse_union_type(s)?)
        } else {
            None
        };
        let default = if s.eat(TokenKind::Eq) {
            Some(super::entry::parse_type(s)?)
        } else {
            None
        };
        let full_range = s.span_from(range);
        params.push(varn_core::ast::TypeParam {
            name,
            constraint,
            default,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect_rangle()?;
    Ok(params)
}

pub(crate) fn parse_fn_type_params(
    s: &mut TokenStream,
) -> Result<Vec<varn_core::ast::TypeParam>, String> {
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        let prange = s.range();

        s.eat(TokenKind::DotDotDot);

        let name = if s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon {
            let n = s.consume_lexeme();
            s.advance();
            n
        } else {
            s.interner.intern("_")
        };
        let ty = super::entry::parse_type(s)?;
        let full_range = s.span_from(prange);
        params.push(varn_core::ast::TypeParam {
            name,
            constraint: Some(ty),
            default: None,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    Ok(params)
}
