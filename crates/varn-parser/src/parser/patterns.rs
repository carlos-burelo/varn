use crate::expressions::{parse_call_args_pub, parse_expr};
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::operators::{Modifiers, Visibility};
use varn_core::ast::{ArrayPatternEl, Decorator, Expr, ExprKind, ObjPatternProp, Param, Pattern};
use varn_core::TokenKind;

pub fn parse_params(s: &mut TokenStream) -> Result<Vec<Param>, String> {
    s.expect(TokenKind::LParen)?;
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        params.push(parse_param(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RParen)?;
    Ok(params)
}

pub fn parse_single_param(s: &mut TokenStream) -> Result<Param, String> {
    parse_param(s)
}

fn parse_param(s: &mut TokenStream) -> Result<Param, String> {
    let range = s.range();
    let mut mods = Modifiers::default();

    loop {
        match s.kind() {
            TokenKind::Public => {
                mods.visibility = Some(Visibility::Public);
                s.advance();
            }
            TokenKind::Private => {
                mods.visibility = Some(Visibility::Private);
                s.advance();
            }
            TokenKind::Protected => {
                mods.visibility = Some(Visibility::Protected);
                s.advance();
            }
            TokenKind::Readonly => {
                mods.is_readonly = true;
                s.advance();
            }
            _ => break,
        }
    }

    let is_rest = s.eat(TokenKind::DotDotDot);
    let pattern = parse_pattern(s)?;
    let is_optional = s.eat(TokenKind::Question);
    let type_ann = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let default = if s.eat(TokenKind::Eq) {
        Some(Box::new(parse_expr(s)?))
    } else {
        None
    };

    let full_range = s.span_from(range);
    Ok(Param {
        pattern,
        type_ann,
        default,
        is_rest,
        is_optional,
        modifiers: mods,
        range: full_range,
    })
}

pub fn parse_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    match s.kind() {
        TokenKind::LBracket => parse_array_pattern(s),
        TokenKind::LBrace => parse_object_pattern(s),
        TokenKind::DotDotDot => {
            s.advance();
            let inner = parse_pattern(s)?;
            let full_range = s.span_from(range);
            Ok(Pattern::Rest {
                argument: Box::new(inner),
                range: full_range,
            })
        }
        TokenKind::Placeholder => {
            s.advance();
            let type_ann = if s.check(TokenKind::Colon) {
                s.advance();
                Some(parse_type(s)?)
            } else {
                None
            };
            let full_range = s.span_from(range);
            Ok(Pattern::Identifier {
                name: String::from("_").into(),
                type_ann,
                range: full_range,
            })
        }
        _ => {
            let name = s.consume_lexeme();
            let type_ann = if s.check(TokenKind::Colon) {
                s.advance();
                Some(parse_type(s)?)
            } else {
                None
            };
            let full_range = s.span_from(range);
            Ok(Pattern::Identifier {
                name,
                type_ann,
                range: full_range,
            })
        }
    }
}

fn parse_array_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    s.advance();
    let mut elements: Vec<Option<ArrayPatternEl>> = vec![];
    let mut rest = None;

    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        if s.check(TokenKind::Comma) {
            return Err(format!(
                "array destructuring holes are not allowed at {}:{}; use `_` to discard a position",
                s.range().start.line,
                s.range().start.column
            ));
        }
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            rest = Some(Box::new(parse_pattern(s)?));
            s.eat(TokenKind::Comma);
            break;
        }
        let pat = parse_pattern(s)?;
        elements.push(Some(ArrayPatternEl { pattern: pat }));
        s.eat(TokenKind::Comma);
    }
    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(range);
    Ok(Pattern::Array {
        elements,
        rest,
        range: full_range,
    })
}

fn parse_object_pattern(s: &mut TokenStream) -> Result<Pattern, String> {
    let range = s.range();
    s.advance();
    let mut properties = vec![];
    let mut rest = None;

    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            rest = Some(Box::new(parse_pattern(s)?));
            s.eat(TokenKind::Comma);
            break;
        }
        let prop_range = s.range();
        let key = s.consume_lexeme();
        let (value, shorthand) = if s.eat(TokenKind::Colon) {
            (parse_pattern(s)?, false)
        } else if s.eat(TokenKind::As) {
            let alias_range = s.range();
            let alias = s.consume_lexeme();
            (
                Pattern::Identifier {
                    name: alias,
                    type_ann: None,
                    range: alias_range,
                },
                false,
            )
        } else {
            (
                Pattern::Identifier {
                    name: key.clone(),
                    type_ann: None,
                    range: prop_range,
                },
                true,
            )
        };
        let value = if s.eat(TokenKind::Eq) {
            let default = parse_expr(s)?;
            let assign_range = s.span_from(prop_range);
            Pattern::Assignment {
                left: Box::new(value),
                right: Box::new(default),
                range: assign_range,
            }
        } else {
            value
        };
        let full_prop_range = s.span_from(prop_range);
        properties.push(ObjPatternProp {
            key,
            value,
            shorthand,
            range: full_prop_range,
        });
        s.eat(TokenKind::Comma);
    }
    s.expect(TokenKind::RBrace)?;
    let full_range = s.span_from(range);
    Ok(Pattern::Object {
        properties,
        rest,
        range: full_range,
    })
}

pub fn parse_decorator_list(s: &mut TokenStream) -> Result<Vec<Decorator>, String> {
    let mut decorators = vec![];
    while s.check(TokenKind::At) {
        let range = s.range();
        s.advance();
        let expr = parse_decorator_expr(s)?;
        let full_range = s.span_from(range);
        decorators.push(Decorator {
            expression: expr,
            range: full_range,
        });
    }
    Ok(decorators)
}

fn parse_decorator_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();
    let name = s.expect_id()?;
    let mut expr = s.expr(range, ExprKind::Identifier { name });

    while s.eat(TokenKind::Dot) {
        let prop_range = s.range();
        let prop = s.consume_lexeme();
        let start_range = *expr.range();
        let prop_expr = s.expr(prop_range, ExprKind::Identifier { name: prop });
        expr = s.expr(
            start_range.to(prop_range),
            ExprKind::Member {
                object: Box::new(expr),
                property: Box::new(prop_expr),
                computed: false,
                optional: false,
            },
        );
    }

    if s.check(TokenKind::LParen) {
        let (type_args, args, call_range) = parse_call_args_pub(s)?;
        let start_range = *expr.range();
        expr = s.expr(
            start_range.to(call_range),
            ExprKind::Call {
                callee: Box::new(expr),
                type_args,
                args,
                optional: false,
            },
        );
    }

    Ok(expr)
}
