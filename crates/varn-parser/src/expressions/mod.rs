mod calls;
mod helpers;
mod ops;
mod primary;

pub use calls::{parse_call_args, parse_call_args_pub, parse_new_callee_expr, parse_unary_expr};

use crate::stream::TokenStream;
use crate::types::parse_type;
use ops::{could_be_arrow, parse_yield_expr, try_parse_arrow};
use varn_core::ast::operators::{AssignOp, BinaryOp, LogicalOp};
use varn_core::ast::{Expr, ExprKind};
use varn_core::TokenKind;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Prec {
    None,
    NullCoalesce,
    LogicalOr,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equality,
    Relational,
    Pipe,
    Range,
    Shift,
    Additive,
    Multiplicative,
    Exponent,
}

fn binary_prec(kind: TokenKind) -> Option<(Prec, bool)> {
    let (p, r) = match kind {
        TokenKind::PipePipe => (Prec::LogicalOr, false),
        TokenKind::AmpAmp => (Prec::LogicalAnd, false),
        TokenKind::Pipe => (Prec::BitwiseOr, false),
        TokenKind::Caret => (Prec::BitwiseXor, false),
        TokenKind::Amp => (Prec::BitwiseAnd, false),
        TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::BangEq | TokenKind::BangEqEq => {
            (Prec::Equality, false)
        }
        TokenKind::LAngle
        | TokenKind::RAngle
        | TokenKind::LtEq
        | TokenKind::GtEq
        | TokenKind::Instanceof
        | TokenKind::In => (Prec::Relational, false),
        TokenKind::PipeGt => (Prec::Pipe, false),
        TokenKind::DotDot | TokenKind::DotDotEq => (Prec::Range, false),
        TokenKind::LtLt | TokenKind::GtGt | TokenKind::GtGtGt => (Prec::Shift, false),
        TokenKind::Plus | TokenKind::Minus => (Prec::Additive, false),
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => (Prec::Multiplicative, false),
        TokenKind::StarStar => (Prec::Exponent, true),
        TokenKind::QuestionQuestion => (Prec::NullCoalesce, false),
        _ => return None,
    };
    Some((p, r))
}

fn token_to_binary_op(kind: TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::Plus => Some(BinaryOp::Add),
        TokenKind::Minus => Some(BinaryOp::Sub),
        TokenKind::Star => Some(BinaryOp::Mul),
        TokenKind::Slash => Some(BinaryOp::Div),
        TokenKind::Percent => Some(BinaryOp::Mod),
        TokenKind::StarStar => Some(BinaryOp::Pow),
        TokenKind::EqEq | TokenKind::EqEqEq => Some(BinaryOp::Eq),
        TokenKind::BangEq | TokenKind::BangEqEq => Some(BinaryOp::NotEq),
        TokenKind::LAngle => Some(BinaryOp::Lt),
        TokenKind::RAngle => Some(BinaryOp::Gt),
        TokenKind::LtEq => Some(BinaryOp::LtEq),
        TokenKind::GtEq => Some(BinaryOp::GtEq),
        TokenKind::Amp => Some(BinaryOp::BitAnd),
        TokenKind::Pipe => Some(BinaryOp::BitOr),
        TokenKind::Caret => Some(BinaryOp::BitXor),
        TokenKind::LtLt => Some(BinaryOp::Shl),
        TokenKind::GtGt => Some(BinaryOp::Shr),
        TokenKind::GtGtGt => Some(BinaryOp::UShr),
        TokenKind::Instanceof => Some(BinaryOp::Instanceof),
        TokenKind::In => Some(BinaryOp::In),
        _ => None,
    }
}

fn token_to_logical_op(kind: TokenKind) -> Option<LogicalOp> {
    match kind {
        TokenKind::AmpAmp => Some(LogicalOp::And),
        TokenKind::PipePipe => Some(LogicalOp::Or),
        TokenKind::QuestionQuestion => Some(LogicalOp::Nullish),
        _ => None,
    }
}

fn token_to_assign_op(kind: TokenKind) -> Option<AssignOp> {
    match kind {
        TokenKind::Eq => Some(AssignOp::Assign),
        TokenKind::PlusEq => Some(AssignOp::AddAssign),
        TokenKind::MinusEq => Some(AssignOp::SubAssign),
        TokenKind::StarEq => Some(AssignOp::MulAssign),
        TokenKind::SlashEq => Some(AssignOp::DivAssign),
        TokenKind::PercentEq => Some(AssignOp::ModAssign),
        TokenKind::StarStarEq => Some(AssignOp::PowAssign),
        TokenKind::AmpEq => Some(AssignOp::BitAndAssign),
        TokenKind::PipeEq => Some(AssignOp::BitOrAssign),
        TokenKind::CaretEq => Some(AssignOp::BitXorAssign),
        TokenKind::LtLtEq => Some(AssignOp::ShlAssign),
        TokenKind::GtGtEq => Some(AssignOp::ShrAssign),
        TokenKind::GtGtGtEq => Some(AssignOp::UShrAssign),
        TokenKind::AmpAmpEq => Some(AssignOp::AndAssign),
        TokenKind::PipePipeEq => Some(AssignOp::OrAssign),
        TokenKind::QuestionQuestionEq => Some(AssignOp::NullishAssign),
        _ => None,
    }
}

pub fn parse_expr(s: &mut TokenStream) -> Result<Expr, String> {
    parse_assign_expr(s)
}

pub fn parse_seq_expr(s: &mut TokenStream) -> Result<Expr, String> {
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
        start.to(*last.range())
    } else {
        start
    };
    Ok(s.expr(range, ExprKind::Sequence { expressions: exprs }))
}

pub(super) fn parse_assign_expr(s: &mut TokenStream) -> Result<Expr, String> {
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
        let range = left.range().to(*right.range());
        return Ok(s.expr(
            range,
            ExprKind::Assign {
                op,
                target: Box::new(left),
                value: Box::new(right),
            },
        ));
    }

    Ok(left)
}

fn parse_conditional_expr(s: &mut TokenStream) -> Result<Expr, String> {
    let expr = parse_binary_expr(s, Prec::None)?;

    if s.eat(TokenKind::Question) {
        let consequent = parse_assign_expr(s)?;
        s.expect(TokenKind::Colon)?;
        let alternate = parse_assign_expr(s)?;
        let range = expr.range().to(*alternate.range());
        return Ok(s.expr(
            range,
            ExprKind::Conditional {
                test: Box::new(expr),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            },
        ));
    }

    Ok(expr)
}

pub(super) fn parse_binary_expr(s: &mut TokenStream, min_prec: Prec) -> Result<Expr, String> {
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

            let range = left.range().to(*right.range());
            if let Some(logical) = token_to_logical_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Logical {
                        op: logical,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                );
            } else if op_kind == TokenKind::DotDot || op_kind == TokenKind::DotDotEq {
                left = s.expr(
                    range,
                    ExprKind::Range {
                        start: Box::new(left),
                        end: Box::new(right),
                        inclusive: op_kind == TokenKind::DotDotEq,
                    },
                );
            } else if op_kind == TokenKind::PipeGt {
                left = s.expr(
                    range,
                    ExprKind::Pipeline {
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                );
            } else if let Some(bin) = token_to_binary_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Binary {
                        op: bin,
                        left: Box::new(left),
                        right: Box::new(right),
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
            let range = left.range().to(ty_range);
            left = if kind == TokenKind::As {
                s.expr(
                    range,
                    ExprKind::As {
                        expression: Box::new(left),
                        type_ann: ty,
                    },
                )
            } else if kind == TokenKind::Is {
                s.expr(
                    range,
                    ExprKind::Is {
                        expression: Box::new(left),
                        type_ann: ty,
                    },
                )
            } else {
                s.expr(
                    range,
                    ExprKind::Satisfies {
                        expression: Box::new(left),
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
