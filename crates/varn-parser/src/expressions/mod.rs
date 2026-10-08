mod arrow_lookahead;
mod arrow_parse;
mod assign_ops;
mod binary_ops;
mod calls;
mod literal_text;
mod logical_ops;
mod precedence;
mod primary;
mod yield_expr;

pub use calls::{parse_call_args, parse_call_args_pub, parse_new_callee_expr, parse_unary_expr};

use assign_ops::token_to_assign_op;
use binary_ops::token_to_binary_op;
use logical_ops::token_to_logical_op;
pub(crate) use precedence::{binary_prec, Prec};

use crate::stream::TokenStream;
use crate::types::parse_type;
use arrow_lookahead::could_be_arrow;
use arrow_parse::try_parse_arrow;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;
use yield_expr::parse_yield_expr;

pub fn parse_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    parse_assign_expr(s)
}

pub fn parse_seq_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start = s.range();
    let first = parse_assign_expr(s)?;
    if !s.check(TokenKind::Comma) {
        return Ok(first);
    }
    let mut exprs = vec![first];
    while s.eat(TokenKind::Comma) {
        exprs.push(parse_assign_expr(s)?);
    }
    let range = if let Some(last) = exprs.last() {
        start.to(s.expr_range(*last))
    } else {
        start
    };
    Ok(s.expr(range, ExprKind::Sequence { expressions: exprs }))
}

pub(super) fn parse_assign_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    if s.check(TokenKind::Yield) {
        return parse_yield_expr(s);
    }

    if could_be_arrow(s) {
        if let Some(arrow) = try_parse_arrow(s)? {
            return Ok(arrow);
        }
    }

    let left = parse_conditional_expr(s)?;

    if let Some(op) = token_to_assign_op(s.kind()) {
        s.advance();
        let right = parse_assign_expr(s)?;
        let range = s.expr_range(left).to(s.expr_range(right));
        return Ok(s.expr(
            range,
            ExprKind::Assign {
                op,
                target: left,
                value: right,
            },
        ));
    }

    Ok(left)
}

fn parse_conditional_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let expr = parse_binary_expr(s, Prec::None)?;

    if s.eat(TokenKind::Question) {
        let consequent = parse_assign_expr(s)?;
        s.expect(TokenKind::Colon)?;
        let alternate = parse_assign_expr(s)?;
        let range = s.expr_range(expr).to(s.expr_range(alternate));
        return Ok(s.expr(
            range,
            ExprKind::Conditional {
                test: expr,
                consequent,
                alternate,
            },
        ));
    }

    Ok(expr)
}

pub(super) fn parse_binary_expr(s: &mut TokenStream, min_prec: Prec) -> Result<ExprId, String> {
    let mut left = parse_unary_expr(s)?;

    loop {
        let kind = s.kind();

        if let Some((prec, right_assoc)) = binary_prec(kind) {
            if prec <= min_prec {
                break;
            }
            let op_kind = kind;
            s.advance();
            let next_min = if right_assoc {
                Prec::Multiplicative
            } else {
                prec
            };
            let right = parse_binary_expr(s, next_min)?;

            let range = s.expr_range(left).to(s.expr_range(right));
            if let Some(logical) = token_to_logical_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Logical {
                        op: logical,
                        left,
                        right,
                    },
                );
            } else if op_kind == TokenKind::DotDot || op_kind == TokenKind::DotDotEq {
                left = s.expr(
                    range,
                    ExprKind::Range {
                        start: left,
                        end: right,
                        inclusive: op_kind == TokenKind::DotDotEq,
                    },
                );
            } else if op_kind == TokenKind::PipeGt {
                left = s.expr(range, ExprKind::Pipeline { left, right });
            } else if let Some(bin) = token_to_binary_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Binary {
                        op: bin,
                        left,
                        right,
                    },
                );
            }
            continue;
        }

        if kind == TokenKind::As
            || kind == TokenKind::Is
            || (kind == TokenKind::Identifier && s.lexeme() == "satisfies")
        {
            s.advance();
            let ty = parse_type(s)?;
            let ty_range = *ty.clone().range();
            let range = s.expr_range(left).to(ty_range);
            left = if kind == TokenKind::As {
                s.expr(
                    range,
                    ExprKind::As {
                        expression: left,
                        type_ann: ty,
                    },
                )
            } else if kind == TokenKind::Is {
                s.expr(
                    range,
                    ExprKind::Is {
                        expression: left,
                        type_ann: ty,
                    },
                )
            } else {
                s.expr(
                    range,
                    ExprKind::Satisfies {
                        expression: left,
                        type_ann: ty,
                    },
                )
            };
            continue;
        }

        break;
    }

    Ok(left)
}
