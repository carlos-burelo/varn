use super::primary::parse_primary_expr;
use super::{parse_assign_expr, parse_expr};
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use varn_core::ast::expr::Arg;
use varn_core::ast::operators::{UnaryOp, UpdateOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::SourceRange;
use varn_core::TokenKind;

/// Parse the property name introduced by `.` or `?.`, called with the dot
/// already consumed.
///
/// A property always sits on the same line as its dot: multi-line chains lead
/// with the dot (`\n  .bar()`), never trail it. So a token on a later line is
/// the *next statement*, not this member's name.
///
/// Taking it anyway — which is what an unconditional `consume()` did — ate the
/// following declaration whole. `const n = g.` followed by `const m = 42`
/// parsed as `Member(g, "const")` plus a bare assignment `m = 42`: `m` was
/// never declared, and the user got `property 'const' does not exist on type
/// 'str'` for code they had not written yet.
///
/// Yields [`ExprKind::Missing`] **without consuming** when there is no name, so
/// the enclosing declaration still parses and still binds its symbols. That is
/// what lets the editor answer `g.<cursor>` from the checker rather than from a
/// token-stream heuristic.
fn parse_property_name(s: &mut TokenStream) -> ExprId {
    if s.is_eof() || s.line() > s.prev_line() {
        // Anchor at the dot, not at the current token: the current token is on
        // the next line and is usually valid code the user did not write wrong.
        let range = s.prev_end_range();
        s.push_error("expected a property name after `.`".to_owned(), range);
        return s.expr(range, ExprKind::Missing);
    }
    let name = s.consume_lexeme();
    let range = s.prev_range();
    s.expr(range, ExprKind::Identifier { name })
}

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
            // The lexer sees a magnitude, never a sign, so the magnitude of
            // i64::MIN (2^63) does not fit the i64 it parses into and the
            // literal is rejected before the minus is ever considered. Folding
            // the sign here is what makes the lower bound of `int` writable.
            //
            // Deliberately narrow: only the one magnitude that is unspellable
            // otherwise is folded, so every program that parses today keeps the
            // exact same AST.
            const I64_MIN_MAGNITUDE: &str = "9223372036854775808";
            s.advance(); // consume the `-`
            if s.kind() == TokenKind::IntegerLiteral
                && s.lexeme().replace('_', "") == I64_MIN_MAGNITUDE
            {
                s.advance(); // consume the magnitude lexeme
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
            // Not the special case: the same body `prefix_unary!` has, minus
            // the `advance` already done above.
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

pub fn parse_new_callee_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let mut expr = parse_primary_expr(s)?;
    loop {
        match s.kind() {
            TokenKind::Dot => {
                s.advance();
                let prop_expr = parse_property_name(s);
                let prop_range = s.expr_range(prop_expr);
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(prop_range),
                    ExprKind::Member {
                        object: expr,
                        property: prop_expr,
                        computed: false,
                        optional: false,
                    },
                );
            }
            TokenKind::LBracket => {
                s.advance();
                let idx = parse_expr(s)?;
                let bracket_tok = s.expect_token(TokenKind::RBracket)?;
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(bracket_tok.range),
                    ExprKind::Member {
                        object: expr,
                        property: idx,
                        computed: true,
                        optional: false,
                    },
                );
            }
            _ => break,
        }
    }
    Ok(expr)
}

fn parse_call_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let mut expr = parse_primary_expr(s)?;

    loop {
        match s.kind() {
            TokenKind::Dot => {
                s.advance();
                let prop_expr = parse_property_name(s);
                let prop_range = s.expr_range(prop_expr);
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(prop_range),
                    ExprKind::Member {
                        object: expr,
                        property: prop_expr,
                        computed: false,
                        optional: false,
                    },
                );
            }
            TokenKind::QuestionDot => {
                s.advance();
                if s.check(TokenKind::LBracket) {
                    s.advance();
                    let idx = parse_expr(s)?;
                    let bracket_tok = s.expect_token(TokenKind::RBracket)?;
                    let start_range = s.expr_range(expr);
                    expr = s.expr(
                        start_range.to(bracket_tok.range),
                        ExprKind::Member {
                            object: expr,
                            property: idx,
                            computed: true,
                            optional: true,
                        },
                    );
                } else if s.check(TokenKind::LParen) {
                    let (type_args, args, call_range) = parse_call_args(s)?;
                    let start_range = s.expr_range(expr);
                    expr = s.expr(
                        start_range.to(call_range),
                        ExprKind::Call {
                            callee: expr,
                            type_args,
                            args,
                            optional: true,
                        },
                    );
                } else {
                    let prop_expr = parse_property_name(s);
                    let prop_range = s.expr_range(prop_expr);
                    let start_range = s.expr_range(expr);
                    expr = s.expr(
                        start_range.to(prop_range),
                        ExprKind::Member {
                            object: expr,
                            property: prop_expr,
                            computed: false,
                            optional: true,
                        },
                    );
                }
            }
            TokenKind::ColonColon => {
                s.advance();
                let prop_expr = parse_property_name(s);
                let prop_name = match &s.arena.expr(prop_expr).kind {
                    ExprKind::Identifier { name } => *name,
                    _ => {
                        let text = s.lexeme().to_owned();
                        s.interner.intern(&text)
                    }
                };
                let prop_range = s.expr_range(prop_expr);
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(prop_range),
                    ExprKind::MetaAccess {
                        target: expr,
                        property: prop_name,
                    },
                );
            }
            TokenKind::LBracket => {
                if s.line() > s.prev_line() {
                    break;
                }
                s.advance();
                let idx = parse_expr(s)?;
                let bracket_tok = s.expect_token(TokenKind::RBracket)?;
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(bracket_tok.range),
                    ExprKind::Member {
                        object: expr,
                        property: idx,
                        computed: true,
                        optional: false,
                    },
                );
            }
            TokenKind::QuestionLBracket => {
                s.advance();
                let idx = parse_expr(s)?;
                let bracket_tok = s.expect_token(TokenKind::RBracket)?;
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(bracket_tok.range),
                    ExprKind::Member {
                        object: expr,
                        property: idx,
                        computed: true,
                        optional: true,
                    },
                );
            }
            TokenKind::LParen => {
                let (type_args, args, call_range) = parse_call_args(s)?;
                let start_range = s.expr_range(expr);
                expr = s.expr(
                    start_range.to(call_range),
                    ExprKind::Call {
                        callee: expr,
                        type_args,
                        args,
                        optional: false,
                    },
                );
            }
            TokenKind::LAngle => {
                if !looks_like_generic_call(s) {
                    break;
                }
                let save = s.save();
                let start_range = s.expr_range(expr);
                match try_parse_generic_call(s, expr, start_range) {
                    Ok(call) => {
                        expr = call;
                    }
                    Err(_) => {
                        s.restore(save);
                        break;
                    }
                }
            }

            TokenKind::Bang => {
                let op_range = s.range();
                s.advance();
                let full_range = s.expr_range(expr).to(op_range);
                expr = s.expr(
                    full_range,
                    ExprKind::NonNull { expression: expr },
                );
            }

            TokenKind::Template | TokenKind::TemplateHead => {
                let template_expr = super::primary::parse_template(s)?;
                let start_range = s.expr_range(expr);
                let full_range = start_range.to(s.expr_range(template_expr));
                expr = s.expr(
                    full_range,
                    ExprKind::TaggedTemplate {
                        tag: expr,
                        template: template_expr,
                    },
                );
            }
            TokenKind::With => {
                s.advance();
                s.expect(TokenKind::LBrace)?;
                let properties = super::primary::parse_object_body(s)?;
                let rbrace_tok = s.expect_token(TokenKind::RBrace)?;
                let start_range = s.expr_range(expr);
                let full_range = start_range.to(rbrace_tok.range);
                expr = s.expr(
                    full_range,
                    ExprKind::With {
                        object: expr,
                        properties,
                    },
                );
            }
            _ => break,
        }
    }

    Ok(expr)
}

fn looks_like_generic_call(s: &TokenStream) -> bool {
    let mut depth = 0i32;
    let mut off = 0usize;
    loop {
        match s.peek_kind(off) {
            TokenKind::LAngle => depth += 1,
            TokenKind::RAngle => {
                depth -= 1;
                if depth == 0 {
                    return s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGt => {
                depth -= 2;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGtGt => {
                depth -= 3;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::EOF
            | TokenKind::Semicolon
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::Eq
            | TokenKind::FatArrow => return false,
            _ => {}
        }
        off += 1;
    }
}


fn try_parse_generic_call(
    s: &mut TokenStream,
    callee: ExprId,
    expr_range: SourceRange,
) -> Result<ExprId, String> {
    let type_args = parse_type_args(s)?;
    if !s.check(TokenKind::LParen) {
        return Err("not a generic call".to_owned());
    }
    let (_, args, call_range) = parse_call_args(s)?;
    Ok(s.expr(
        expr_range.to(call_range),
        ExprKind::Call {
            callee,
            type_args,
            args,
            optional: false,
        },
    ))
}

pub fn parse_call_args(
    s: &mut TokenStream,
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
    s.expect(TokenKind::LParen)?;
    let mut args = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            args.push(Arg::Spread(parse_assign_expr(s)?));
        } else if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Colon {
            let label = s.consume_lexeme();
            s.advance();
            let label = s.interner.resolve(label).to_string();
            args.push(Arg::Named {
                label,
                value: parse_assign_expr(s)?,
            });
        } else {
            args.push(Arg::Positional(parse_assign_expr(s)?));
        }
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    let rparen = s.expect_token(TokenKind::RParen)?;
    Ok((vec![], args, rparen.range))
}

pub fn parse_call_args_pub(
    s: &mut TokenStream,
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
    parse_call_args(s)
}
