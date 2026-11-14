use crate::error::PResult;
use crate::stream::TokenStream;
use vn_core::ast::TypeNode;
use vn_core::TokenKind;

pub fn parse_type_args(s: &mut TokenStream) -> PResult<Vec<TypeNode>> {
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

pub fn parse_type_params(s: &mut TokenStream) -> PResult<Vec<vn_core::ast::TypeParam>> {
    s.expect(TokenKind::LAngle)?;
    let mut params = vec![];
    while !s.check(TokenKind::RAngle) && !s.is_eof() {
        let range = s.range();
        let variance = parse_declared_variance(s);
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
        params.push(vn_core::ast::TypeParam {
            variance,
            name,
            constraint,
            default,
            is_rest: false,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect_rangle()?;
    Ok(params)
}

pub(crate) fn parse_fn_type_params(s: &mut TokenStream) -> PResult<Vec<vn_core::ast::TypeParam>> {
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        let prange = s.range();

        let is_rest = s.eat(TokenKind::DotDotDot);

        let name = if s.kind() == TokenKind::Identifier && s.peek_kind(1) == TokenKind::Colon {
            let n = s.consume_lexeme();
            s.advance();
            n
        } else {
            s.interner.intern(vn_core::TokenKind::Placeholder.as_str())
        };
        let ty = super::entry::parse_type(s)?;
        let full_range = s.span_from(prange);
        params.push(vn_core::ast::TypeParam {
            variance: None,
            name,
            constraint: Some(ty),
            default: None,
            is_rest,
            range: full_range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    Ok(params)
}

fn parse_declared_variance(s: &mut TokenStream) -> Option<vn_core::ast::DeclaredVariance> {
    let names_a_parameter = matches!(s.peek_kind(1), TokenKind::Identifier);
    let kind = s.kind();
    let variance = if names_a_parameter && kind == TokenKind::Out {
        vn_core::ast::DeclaredVariance::Out
    } else if names_a_parameter && kind == TokenKind::In {
        vn_core::ast::DeclaredVariance::In
    } else {
        return None;
    };
    s.advance();
    Some(variance)
}
