use super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{Expr, ExprKind, Param, Pattern};
use varn_core::TokenKind;

pub(super) fn could_be_arrow(s: &TokenStream) -> bool {
    let k = s.kind();
    if k == TokenKind::LParen {
        // Fast scan: look for closing ) followed by => (or : for return type then =>).
        // Avoids full speculative parse for ordinary grouped expressions.
        return paren_leads_to_arrow(s);
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

fn paren_leads_to_arrow(s: &TokenStream) -> bool {
    let mut depth = 0i32;
    let mut off = 0usize;
    loop {
        match s.peek_kind(off) {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    // After the closing paren: optional `: type`, then `=>`
                    let mut next_off = off + 1;
                    if s.peek_kind(next_off) == TokenKind::Colon {
                        // skip over return type annotation tokens until we hit => or something else
                        next_off += 1;
                        let mut type_depth = 0i32;
                        loop {
                            match s.peek_kind(next_off) {
                                TokenKind::LAngle | TokenKind::LBracket | TokenKind::LParen => {
                                    type_depth += 1;
                                }
                                TokenKind::RAngle | TokenKind::RBracket | TokenKind::RParen => {
                                    type_depth -= 1;
                                }
                                TokenKind::FatArrow if type_depth == 0 => return true,
                                TokenKind::EOF
                                | TokenKind::Semicolon
                                | TokenKind::LBrace
                                | TokenKind::RBrace => return false,
                                _ => {}
                            }
                            next_off += 1;
                            if next_off > 128 {
                                return true; // give up, let speculative parse decide
                            }
                        }
                    }
                    return s.peek_kind(next_off) == TokenKind::FatArrow;
                }
            }
            TokenKind::EOF | TokenKind::Semicolon => return false,
            _ => {}
        }
        off += 1;
        if off > 128 {
            return true; // give up, let speculative parse decide
        }
    }
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
