use super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{ExprId, ExprKind, Param, Pattern};
use varn_core::TokenKind;

pub(super) fn try_parse_arrow(s: &mut TokenStream) -> Result<Option<ExprId>, String> {
    let save = s.save();
    match parse_arrow_attempt(s) {
        Ok(expr) => Ok(Some(expr)),
        Err(_) => {
            s.restore(save);
            Ok(None)
        }
    }
}

fn parse_arrow_attempt(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();
    let is_async = s.eat(TokenKind::Async);

    let params = if s.check(TokenKind::LParen) {
        crate::parser::parse_params(s)?
    } else {
        let param_text = s.lexeme().to_owned();
        let param_name = s.interner.intern(&param_text);
        let tok = s.expect_token(TokenKind::Identifier)?;
        let param_range = tok.range;
        vec![Param {
            pattern: Pattern::Identifier {
                name: param_name,
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

    let full_range = s.span_from(start_range);
    Ok(s.expr(
        full_range,
        ExprKind::Arrow {
            params,
            return_type,
            body: Box::new(body),
            is_async,
        },
    ))
}
