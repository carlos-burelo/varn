use crate::stream::TokenStream;
use varn_core::ast::expr::MatchCase;
use varn_core::ast::{Expr, MatchBody, MatchPattern};
use varn_core::TokenKind;

pub(super) fn parse_match_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    s.advance();

    let subject = if s.eat(TokenKind::LParen) {
        let e = super::super::parse_expr(s)?;
        s.expect(TokenKind::RParen)?;
        e
    } else {
        super::super::parse_binary_expr(s, super::super::Prec::None)?
    };

    s.expect(TokenKind::LBrace)?;
    let mut cases = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        cases.extend(parse_match_case(s)?);
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(Expr::new_with_range(
        full_range,
        varn_core::ast::ExprKind::Match {
            subject: Box::new(subject),
            cases,
        },
    ))
}

fn parse_match_case(s: &mut TokenStream) -> Result<Vec<MatchCase>, String> {
    let range = s.range();

    let mut patterns = vec![parse_match_pattern(s)?];
    while s.eat(TokenKind::Pipe) {
        patterns.push(parse_match_pattern(s)?);
    }

    let guard = if s.eat(TokenKind::If) {
        Some(super::super::parse_expr(s)?)
    } else {
        None
    };
    s.expect(TokenKind::FatArrow)?;
    let body = if s.check(TokenKind::LBrace) {
        MatchBody::Block(crate::parser::parse_block(s)?)
    } else {
        MatchBody::Expr(super::super::parse_expr(s)?)
    };
    s.eat(TokenKind::Comma);

    let full_case_range = s.span_from(range);
    let cases = patterns
        .into_iter()
        .map(|pattern| MatchCase {
            pattern,
            guard: guard.clone(),
            body: body.clone(),
            range: full_case_range,
        })
        .collect();
    Ok(cases)
}

fn parse_match_pattern(s: &mut TokenStream) -> Result<MatchPattern, String> {
    match s.kind() {
        TokenKind::Placeholder => {
            s.advance();
            Ok(MatchPattern::Wildcard)
        }
        TokenKind::Identifier => parse_identifier_match_pattern(s),
        _ => {
            let expr = super::parse_primary_expr(s)?;
            Ok(MatchPattern::Literal(expr))
        }
    }
}

fn parse_identifier_match_pattern(s: &mut TokenStream) -> Result<MatchPattern, String> {
    let id_range = s.range();
    let name = s.consume_lexeme();
    if s.check(TokenKind::LParen) {
        return parse_variant_tuple_pattern(s, name.to_string());
    }
    if s.check(TokenKind::LBrace) {
        return parse_variant_record_pattern(s, name.to_string());
    }

    if s.check(TokenKind::Dot) {
        let id_expr = Expr::new_with_range(
            id_range,
            varn_core::ast::ExprKind::Identifier { name: name.clone() },
        );
        s.advance();
        let prop_name = s.lexeme().to_owned();
        let prop_tok = s.consume();
        let prop_range = prop_tok.range;
        let expr = Expr::new_with_range(
            id_range.to(prop_range),
            varn_core::ast::ExprKind::Member {
                object: Box::new(id_expr),
                property: Box::new(Expr::new_with_range(
                    prop_range,
                    varn_core::ast::ExprKind::Identifier {
                        name: prop_name.into(),
                    },
                )),
                computed: false,
                optional: false,
            },
        );
        return Ok(MatchPattern::Literal(expr));
    }
    Ok(MatchPattern::Identifier(name))
}

fn parse_variant_tuple_pattern(
    s: &mut TokenStream,
    enum_name: String,
) -> Result<MatchPattern, String> {
    use varn_core::ast::MatchBinding;
    let variant_name = enum_name.clone();
    s.advance();
    let mut bindings: Vec<MatchBinding> = Vec::new();
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        let range = s.range();
        if s.check(TokenKind::Placeholder) {
            s.advance();
            bindings.push(MatchBinding {
                name: "_".into(),
                range,
            });
        } else if s.check(TokenKind::Identifier) {
            let name = s.consume_lexeme();
            if s.eat(TokenKind::Colon) {
                parse_match_pattern(s)?;
            }
            bindings.push(MatchBinding {
                name: name.into(),
                range,
            });
        } else {
            break;
        }
        if s.check(TokenKind::Comma) {
            s.advance();
        }
    }
    s.expect(TokenKind::RParen)?;
    Ok(MatchPattern::EnumVariant {
        enum_name: enum_name.into(),
        variant_name: variant_name.into(),
        bindings,
    })
}

fn parse_variant_record_pattern(
    s: &mut TokenStream,
    enum_name: String,
) -> Result<MatchPattern, String> {
    use varn_core::ast::MatchBinding;
    let variant_name = enum_name.clone();
    s.advance();
    let mut bindings: Vec<MatchBinding> = Vec::new();
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        let range = s.range();
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            break;
        }
        if s.check(TokenKind::Placeholder) {
            s.advance();
            bindings.push(MatchBinding {
                name: "_".into(),
                range,
            });
        } else if s.check(TokenKind::Identifier) {
            let name = s.consume_lexeme();
            if s.eat(TokenKind::Colon) {
                parse_match_pattern(s)?;
            }
            bindings.push(MatchBinding {
                name: name.into(),
                range,
            });
        } else {
            break;
        }
        if s.check(TokenKind::Comma) {
            s.advance();
        }
    }
    s.expect(TokenKind::RBrace)?;
    Ok(MatchPattern::EnumVariant {
        enum_name: enum_name.into(),
        variant_name: variant_name.into(),
        bindings,
    })
}
