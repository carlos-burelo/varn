use crate::error::{PResult, ParseError};
use super::parse_assign_expr;
use crate::stream::TokenStream;
use varn_core::ast::{ExprKind, Pattern, StmtId, StmtKind, VarKind};
use varn_core::TokenKind;

pub(crate) fn parse_yield_stmt(s: &mut TokenStream) -> PResult<StmtId> {
    let start_range = s.range();
    s.advance();
    let delegate = s.eat(TokenKind::Star);
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(parse_assign_expr(s)?)
    } else {
        None
    };
    s.eat_semicolon();
    let range = s.span_from(start_range);
    let Some(source) = argument.filter(|_| delegate) else {
        if delegate {
            return Err(ParseError::invalid_expression(String::from(
                "`yield*` needs the iterable it delegates to",
            )));
        }
        let yielded = s.expr(range, ExprKind::Yield { argument });
        return Ok(s.stmt(
            range,
            StmtKind::Expr {
                expression: yielded,
            },
        ));
    };
    let item = s.interner.intern(varn_core::intrinsics::DELEGATED_ITEM);
    let item_expr = s.expr(range, ExprKind::Identifier { name: item });
    let yielded = s.expr(
        range,
        ExprKind::Yield {
            argument: Some(item_expr),
        },
    );
    let forward = s.stmt(
        range,
        StmtKind::Expr {
            expression: yielded,
        },
    );
    let body = s.stmt(
        range,
        StmtKind::Block {
            stmts: vec![forward],
        },
    );
    Ok(s.stmt(
        range,
        StmtKind::ForOf {
            kind: VarKind::Const,
            left: Pattern::Identifier { name: item, range },
            right: source,
            body,
            is_await: s.async_body,
        },
    ))
}
