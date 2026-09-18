use crate::stream::TokenStream;
use varn_core::ast::expr::MatchCase;
use varn_core::ast::{ExprId, MatchBody, MatchPattern};
use varn_core::TokenKind;

pub(super) fn parse_match_expr(s: &mut TokenStream) -> Result<ExprId, String> {
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
    Ok(s.expr(
        full_range,
        varn_core::ast::ExprKind::Match { subject, cases },
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
        return parse_variant_tuple_pattern(s, name);
    }
    if s.check(TokenKind::LBrace) {
        return parse_variant_record_pattern(s, name);
    }

    if s.check(TokenKind::Dot) {
        let id_expr = s.expr(id_range, varn_core::ast::ExprKind::Identifier { name });
        s.advance();
        let prop_name = s.consume_lexeme();
        let prop_range = s.prev_range();
        let prop_expr = s.expr(
            prop_range,
            varn_core::ast::ExprKind::Identifier { name: prop_name },
        );
        let expr = s.expr(
            id_range.to(prop_range),
            varn_core::ast::ExprKind::Member {
                object: id_expr,
                property: prop_expr,
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
    enum_name: varn_core::Atom,
) -> Result<MatchPattern, String> {
    use varn_core::ast::MatchBinding;
    let variant_name = enum_name;
    s.advance();
    let mut bindings: Vec<MatchBinding> = Vec::new();
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        let range = s.range();
        if s.check(TokenKind::Placeholder) {
            s.advance();
            bindings.push(MatchBinding {
                name: s.interner.intern("_"),
                range,
            });
        } else if s.check(TokenKind::Identifier) {
            let name = s.consume_lexeme();
            if s.eat(TokenKind::Colon) {
                parse_match_pattern(s)?;
            }
            bindings.push(MatchBinding { name, range });
        } else {
            return Err(format!(
                "expected binding name or `_` in variant pattern, got {:?}",
                s.kind()
            ));
        }
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RParen)?;
    Ok(MatchPattern::EnumVariant {
        enum_name,
        variant_name,
        bindings,
    })
}

fn parse_variant_record_pattern(
    s: &mut TokenStream,
    name: varn_core::Atom,
) -> Result<MatchPattern, String> {
    use varn_core::ast::MatchBinding;
    s.advance();
    // `Variant { x, y }` — a variant pattern whose payload is destructured by
    // field name. Bindings are collected in written order, which matches the
    // variant's declared field order for the common case.
    let mut bindings: Vec<MatchBinding> = Vec::new();
    let mut rest = false;
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.eat(TokenKind::DotDotDot) {
            rest = true;
            break;
        }
        let range = s.range();
        let field_name = s.expect_id()?;
        if s.eat(TokenKind::Colon) {
            parse_match_pattern(s)?;
        }
        bindings.push(MatchBinding {
            name: field_name,
            range,
        });
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RBrace)?;
    let _ = rest;
    Ok(MatchPattern::EnumVariant {
        enum_name: name,
        variant_name: name,
        bindings,
    })
}
