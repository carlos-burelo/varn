use super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{Expr, ExprKind, Param, Pattern};
use varn_core::TokenKind;

pub(super) fn could_be_arrow(s: &TokenStream) -> bool {
    let k = s.kind();
    if k == TokenKind::LParen {
        return true;
    }
    if k == TokenKind::Identifier && s.peek_kind(1) == TokenKind::FatArrow {
        return true;
    }
    if k == TokenKind::Async {
        let k2 = s.peek_kind(1);
        if k2 == TokenKind::Identifier || k2 == TokenKind::LParen {
            return true;
        }
    }
    false
}

pub(super) fn try_parse_arrow(s: &mut TokenStream) -> Result<Option<Expr>, String> {
    let save = s.save();
    match parse_arrow_attempt(s) {
        Ok(expr) => Ok(Some(expr)),
        Err(_) => {
            s.restore(save);
            Ok(None)
        }
    }
}

fn parse_arrow_attempt(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    let is_async = s.eat(TokenKind::Async);

    let params = if s.check(TokenKind::LParen) {
        crate::parser::parse_params(s)?
    } else {
        let tok = s.expect_token(TokenKind::Identifier)?;
        let param_range = tok.range;
        vec![Param {
            pattern: Pattern::Identifier {
                name: tok.lexeme.to_string().into(),
                type_ann: None,
                range: param_range,
            },
            type_ann: None,
            default: None,
            is_rest: false,
            is_optional: false,
            modifiers: Default::default(),
            range: param_range,
        }]
    };

    let return_type = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    s.expect(TokenKind::FatArrow)?;

    let body = if s.check(TokenKind::LBrace) {
        ArrowBody::Block(crate::parser::parse_block(s)?)
    } else {
        ArrowBody::Expr(parse_assign_expr(s)?)
    };

    Ok(Expr::new_with_range(
        range,
        ExprKind::Arrow {
            params,
            return_type,
            body: Box::new(body),
            is_async,
        },
    ))
}

pub(super) fn parse_yield_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    s.advance();
    let delegate = s.eat(TokenKind::Star);
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(Box::new(parse_assign_expr(s)?))
    } else {
        None
    };
    Ok(Expr::new_with_range(
        range,
        ExprKind::Yield { argument, delegate },
    ))
}
