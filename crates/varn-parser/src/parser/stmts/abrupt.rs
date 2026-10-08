use super::block::parse_block;
use crate::stream::TokenStream;
use varn_core::ast::{CatchClause, StmtId, StmtKind, VarDeclarator};
use varn_core::TokenKind;

pub(super) fn parse_using_stmt(s: &mut TokenStream, is_await: bool) -> Result<StmtId, String> {
    let start_range = s.range();
    if is_await {
        s.expect(TokenKind::Await)?;
    }
    s.expect(TokenKind::Using)?;

    let mut declarations = Vec::new();
    loop {
        let decl_start = s.range();
        let id = super::super::patterns::parse_pattern(s)?;
        let type_ann = if s.eat(TokenKind::Colon) {
            Some(crate::types::parse_type(s)?)
        } else {
            None
        };
        s.expect(TokenKind::Eq)?;
        let init = Some(crate::expressions::parse_seq_expr(s)?);
        let decl_range = s.span_from(decl_start);

        declarations.push(VarDeclarator {
            id,
            type_ann,
            init,
            range: decl_range,
        });

        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.eat_semicolon();

    let range = s.span_from(start_range);
    Ok(s.stmt(
        range,
        StmtKind::Using {
            declarations,
            is_await,
        },
    ))
}

pub(super) fn parse_return_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(crate::expressions::parse_seq_expr(s)?)
    } else {
        None
    };
    s.eat_semicolon();
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::Return { argument }))
}

pub(super) fn parse_break_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();

    let label = if s.check(TokenKind::Identifier)
        && !s.check(TokenKind::Semicolon)
        && s.peek_line(0) == s.prev_line()
    {
        Some(s.consume_lexeme())
    } else {
        None
    };
    s.eat_semicolon();
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::Break { label }))
}

pub(super) fn parse_continue_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();

    let label = if s.check(TokenKind::Identifier)
        && !s.check(TokenKind::Semicolon)
        && s.peek_line(0) == s.prev_line()
    {
        Some(s.consume_lexeme())
    } else {
        None
    };
    s.eat_semicolon();
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::Continue { label }))
}

pub(super) fn parse_throw_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    let argument = crate::expressions::parse_seq_expr(s)?;
    s.eat_semicolon();
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::Throw { argument }))
}

pub(super) fn parse_try_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    let block = parse_block(s)?;

    let mut catches = Vec::new();
    while s.eat(TokenKind::Catch) {
        let catch_start = s.range();
        let (param, type_ann) = if s.eat(TokenKind::LParen) {
            let p = super::super::patterns::parse_pattern(s)?;
            let ty = if s.eat(TokenKind::Colon) {
                Some(crate::types::parse_type(s)?)
            } else {
                None
            };
            s.expect(TokenKind::RParen)?;
            (Some(p), ty)
        } else {
            (None, None)
        };
        let body = parse_block(s)?;
        let catch_range = s.span_from(catch_start);
        catches.push(CatchClause {
            param,
            type_ann,
            body,
            range: catch_range,
        });
    }

    let finally = if s.eat(TokenKind::Finally) {
        Some(parse_block(s)?)
    } else {
        None
    };

    if catches.is_empty() && finally.is_none() {
        return Err("expected `catch` or `finally` after `try` block".to_owned());
    }

    let range = s.span_from(start_range);
    Ok(s.stmt(
        range,
        StmtKind::Try {
            block,
            catches,
            finally,
        },
    ))
}
