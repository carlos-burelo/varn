use super::trailer::parse_call_expr;
use crate::stream::TokenStream;
use varn_core::ast::operators::{UnaryOp, UpdateOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

pub fn parse_unary_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();

    macro_rules! prefix_unary {
        ($op:expr) => {{
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Unary {
                    op: $op,
                    prefix: true,
                    operand: o,
                },
            ))
        }};
    }

    match s.kind() {
        TokenKind::Bang => prefix_unary!(UnaryOp::Not),
        TokenKind::Tilde => prefix_unary!(UnaryOp::BitNot),
        TokenKind::Minus => {
            const I64_MIN_MAGNITUDE: &str = "9223372036854775808";
            s.advance();
            if s.kind() == TokenKind::IntegerLiteral
                && s.lexeme().replace('_', "") == I64_MIN_MAGNITUDE
            {
                s.advance();
                let full_range = s.span_from(start_range);
                let raw = s.interner.intern(&format!("-{}", I64_MIN_MAGNITUDE));
                return Ok(s.expr(
                    full_range,
                    ExprKind::IntLiteral {
                        value: i64::MIN,
                        raw,
                    },
                ));
            }

            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Unary {
                    op: UnaryOp::Minus,
                    prefix: true,
                    operand: o,
                },
            ))
        }
        TokenKind::Plus => prefix_unary!(UnaryOp::Plus),
        TokenKind::Typeof => prefix_unary!(UnaryOp::Typeof),
        TokenKind::Void => Err("`void` is not supported; use `_` to discard values".to_owned()),
        TokenKind::Delete => Err("`delete` is not supported".to_owned()),
        TokenKind::Await => {
            s.advance();
            let argument = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(full_range, ExprKind::Await { argument }))
        }
        TokenKind::Try => {
            s.advance();
            let operand = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Try {
                    expression: operand,
                },
            ))
        }
        TokenKind::PlusPlus => {
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Update {
                    op: UpdateOp::Increment,
                    prefix: true,
                    operand: o,
                },
            ))
        }
        TokenKind::MinusMinus => {
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Update {
                    op: UpdateOp::Decrement,
                    prefix: true,
                    operand: o,
                },
            ))
        }
        _ => parse_postfix_expr(s),
    }
}

fn parse_postfix_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let mut expr = parse_call_expr(s)?;

    loop {
        let op_range = s.range();
        match s.kind() {
            TokenKind::PlusPlus => {
                s.advance();
                let full_range = s.expr_range(expr).to(op_range);
                expr = s.expr(
                    full_range,
                    ExprKind::Update {
                        op: UpdateOp::Increment,
                        prefix: false,
                        operand: expr,
                    },
                );
            }
            TokenKind::MinusMinus => {
                s.advance();
                let full_range = s.expr_range(expr).to(op_range);
                expr = s.expr(
                    full_range,
                    ExprKind::Update {
                        op: UpdateOp::Decrement,
                        prefix: false,
                        operand: expr,
                    },
                );
            }
            _ => break,
        }
    }

    Ok(expr)
}
