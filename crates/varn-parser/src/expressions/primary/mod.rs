mod match_expr;
mod object;
mod template;

use super::helpers::{parse_int_radix, split_regex, unescape_string};
use super::{parse_call_args, parse_seq_expr, try_parse_arrow};
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use std::rc::Rc;
use varn_core::ast::{ArrayEl, Expr, ExprKind};
use varn_core::ParsedNumber;
use varn_core::TokenKind;

use self::match_expr::parse_match_expr;
use self::object::parse_object_expr;
pub(crate) use self::object::parse_object_body;
pub(crate) use self::template::parse_template;

pub fn parse_primary_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let range = s.range();

    match s.kind() {
        TokenKind::IntegerLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral
        | TokenKind::HexLiteral => {
            let pre_parsed = s.parsed_num();
            let raw = s.consume_lexeme();
            let value: i64 = match pre_parsed {
                Some(ParsedNumber::Int(v)) => v,
                _ => parse_int_radix(&raw)
                    .ok_or_else(|| format!("integer literal `{}` overflows i64", raw))?,
            };
            Ok(s.expr(
                range,
                ExprKind::IntLiteral {
                    value,
                    raw,
                },
            ))
        }
        TokenKind::FloatLiteral => {
            let pre_parsed = s.parsed_num();
            let raw = s.consume_lexeme();
            let value: f64 = match pre_parsed {
                Some(ParsedNumber::Float(v)) => v,
                _ => raw
                    .parse()
                    .map_err(|_| format!("invalid float literal: {}", raw))?,
            };
            Ok(s.expr(
                range,
                ExprKind::FloatLiteral {
                    value,
                    raw,
                },
            ))
        }
        TokenKind::BigIntLiteral => {
            let raw = s.consume_lexeme();
            Ok(s.expr(
                range,
                ExprKind::BigIntLiteral {
                    raw,
                },
            ))
        }
        TokenKind::DecimalLiteral => {
            let raw = s.consume_lexeme();
            Ok(s.expr(
                range,
                ExprKind::DecimalLiteral {
                    raw,
                },
            ))
        }
        TokenKind::RawStr => {
            let value = s.consume_lexeme().to_string();
            Ok(s.expr(
                range,
                ExprKind::StrLiteral {
                    value,
                },
            ))
        }
        TokenKind::Str => {
            let value = unescape_string(s.lexeme());
            s.advance();
            Ok(s.expr(
                range,
                ExprKind::StrLiteral {
                    value,
                },
            ))
        }
        TokenKind::Char => {
            let ch = unescape_string(s.lexeme()).chars().next().unwrap_or('\0');
            s.advance();
            Ok(s.expr(
                range,
                ExprKind::CharLiteral { value: ch },
            ))
        }
        TokenKind::True => {
            s.advance();
            Ok(s.expr(
                range,
                ExprKind::BoolLiteral { value: true },
            ))
        }
        TokenKind::False => {
            s.advance();
            Ok(s.expr(
                range,
                ExprKind::BoolLiteral { value: false },
            ))
        }
        TokenKind::Null => {
            s.advance();
            Ok(s.expr(range, ExprKind::NullLiteral))
        }
        TokenKind::RegularExpression => {
            let raw = s.consume_lexeme();
            let (pattern, flags) = split_regex(&raw);
            Ok(s.expr(
                range,
                ExprKind::RegexLiteral {
                    pattern,
                    flags,
                },
            ))
        }

        TokenKind::Template | TokenKind::TemplateHead => parse_template(s),

        TokenKind::Identifier => {
            let name = s.consume_lexeme();
            Ok(s.expr(
                range,
                ExprKind::Identifier { name },
            ))
        }
        TokenKind::Placeholder => {
            s.advance();
            Ok(s.expr(
                range,
                ExprKind::Identifier {
                    name: Rc::from("_"),
                },
            ))
        }

        TokenKind::This => {
            s.advance();
            Ok(s.expr(range, ExprKind::This))
        }
        TokenKind::Super => {
            s.advance();
            Ok(s.expr(range, ExprKind::Super))
        }

        TokenKind::LBracket => parse_array_expr(s),
        TokenKind::LBrace => parse_object_expr(s),

        TokenKind::LParen => {
            let start_range = s.range();
            let pos_before_lparen = s.save();
            s.advance();
            if s.check(TokenKind::RParen) {
                s.restore(pos_before_lparen);
                return Err("unit paren — should be handled by arrow parser".to_owned());
            }
            let expr = parse_seq_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Paren {
                    expression: Box::new(expr),
                },
            ))
        }

        TokenKind::New => parse_new_expr(s, range),
        TokenKind::Function => parse_function_expr(s),

        TokenKind::Async => {
            let save = s.save();
            if let Ok(Some(arrow)) = try_parse_arrow(s) {
                return Ok(arrow);
            }
            s.restore(save);
            let start_range = s.range();
            s.advance();
            s.expect(TokenKind::Function)?;
            parse_function_expr_inner_with_start(s, true, start_range)
        }

        TokenKind::Class => parse_class_expr(s),
        TokenKind::Match => parse_match_expr(s),

        TokenKind::Hash => {
            let start_range = s.range();
            if s.peek_kind(1) == TokenKind::LBracket {
                s.advance();
                s.advance();
                let mut elements = vec![];
                while !s.check(TokenKind::RBracket) && !s.is_eof() {
                    elements.push(super::parse_assign_expr(s)?);
                    s.eat(TokenKind::Comma);
                }
                s.expect(TokenKind::RBracket)?;
                let full_range = s.span_from(start_range);
                Ok(s.expr(full_range, ExprKind::Tuple { elements }))
            } else if s.peek_kind(1) == TokenKind::LBrace {
                s.advance();
                let obj = parse_object_expr(s)?;
                let full_range = s.span_from(start_range);
                let ExprKind::Object { properties } = obj.kind else {
                    unreachable!()
                };
                Ok(s.expr(full_range, ExprKind::Record { properties }))
            } else {
                Err(format!("Unexpected `#` at {}:{}", s.line(), s.column()))
            }
        }

        _ => {
            let kind = s.kind();
            if kind.can_be_identifier() {
                let name = s.consume_lexeme();
                Ok(s.expr(range, ExprKind::Identifier { name }))
            } else {
                Err(format!(
                    "Unexpected token {:?} ({:?}) in expression at {}:{}",
                    s.kind(),
                    s.lexeme(),
                    s.line(),
                    s.column()
                ))
            }
        }
    }
}

fn parse_array_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let start_range = s.range();
    s.advance();
    let mut elements = vec![];

    while !s.check(TokenKind::RBracket) && !s.is_eof() {
        if s.check(TokenKind::Comma) {
            elements.push(ArrayEl::Hole);
            s.advance();
            continue;
        }
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            elements.push(ArrayEl::Spread(super::parse_assign_expr(s)?));
        } else {
            elements.push(ArrayEl::Expr(super::parse_assign_expr(s)?));
        }
        s.eat(TokenKind::Comma);
    }

    s.expect(TokenKind::RBracket)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(full_range, ExprKind::Array { elements }))
}

fn parse_new_expr(s: &mut TokenStream, range: varn_core::SourceRange) -> Result<Expr, String> {
    s.advance();
    let callee = super::parse_new_callee_expr(s)?;
    let mut type_args = vec![];
    if s.check(TokenKind::LAngle) {
        let save = s.save();
        match parse_type_args(s) {
            Ok(ta) if s.check(TokenKind::LParen) => {
                type_args = ta;
            }
            _ => {
                s.restore(save);
            }
        }
    }
    let args = if s.check(TokenKind::LParen) {
        let (_, a, _) = parse_call_args(s)?;
        a
    } else {
        vec![]
    };
    let full_range = s.span_from(range);
    Ok(s.expr(
        full_range,
        ExprKind::New {
            callee: Box::new(callee),
            type_args,
            args,
        },
    ))
}

fn parse_function_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let start_range = s.range();
    s.advance();
    parse_function_expr_inner_with_start(s, false, start_range)
}

fn parse_function_expr_inner_with_start(
    s: &mut TokenStream,
    is_async: bool,
    start_range: varn_core::SourceRange,
) -> Result<Expr, String> {
    let is_generator = s.eat(TokenKind::Star);
    let id = if s.check(TokenKind::Identifier) {
        Some(s.consume_lexeme())
    } else {
        None
    };
    let params = crate::parser::parse_params(s)?;
    let return_type = if s.eat(TokenKind::Colon) {
        Some(crate::types::parse_type(s)?)
    } else {
        None
    };
    let body = crate::parser::parse_block(s)?;
    let full_range = s.span_from(start_range);
    Ok(s.expr(
        full_range,
        ExprKind::Function {
            fn_id: id,
            params,
            return_type,
            body: Box::new(body),
            is_async,
            is_generator,
        },
    ))
}

fn parse_class_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let decl = crate::parser::parse_class_decl(s, vec![], false)?;
    let full_range = decl.range;
    Ok(s.expr(
        full_range,
        ExprKind::ClassExpr {
            declaration: Box::new(decl),
        },
    ))
}
