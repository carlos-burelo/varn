use super::super::{parse_call_args, parse_new_callee_expr};
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

pub(super) fn parse_new_expr(
    s: &mut TokenStream,
    range: varn_core::SourceRange,
) -> Result<ExprId, String> {
    s.advance();
    let callee = parse_new_callee_expr(s)?;
    let mut type_args = vec![];
    if s.check(TokenKind::LAngle) {
        let save = s.save();
        match parse_type_args(s) {
            Ok(ta) if s.check(TokenKind::LParen) => {
                type_args = ta;
            }
            Ok(_) | Err(_) => {
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
            callee,
            type_args,
            args,
        },
    ))
}
