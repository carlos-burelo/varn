use super::super::primary::parse_primary_expr;
use super::super::parse_expr;
use super::generic_args::{looks_like_generic_call, parse_call_args, try_parse_generic_call};
use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

fn parse_property_name(s: &mut TokenStream) -> ExprId {
    if s.is_eof() || s.line() > s.prev_line() {
        let range = s.prev_end_range();
        s.push_error("expected a property name after `.`".to_owned(), range);
        return s.expr(range, ExprKind::Missing);
    }
    let name = s.consume_lexeme();
    let range = s.prev_range();
    s.expr(range, ExprKind::Identifier { name })
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

pub(super) fn parse_call_expr(s: &mut TokenStream) -> Result<ExprId, String> {
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
                expr = s.expr(full_range, ExprKind::NonNull { expression: expr });
            }

            TokenKind::Template | TokenKind::TemplateHead => {
                let template_expr = super::super::primary::parse_template(s)?;
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
                let properties = super::super::primary::parse_object_body(s)?;
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
