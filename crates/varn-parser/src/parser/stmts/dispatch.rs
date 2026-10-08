use super::abrupt::{
    parse_break_stmt, parse_continue_stmt, parse_return_stmt, parse_throw_stmt, parse_try_stmt,
    parse_using_stmt,
};
use super::branches::{
    parse_do_while_stmt, parse_for_stmt, parse_if_stmt, parse_switch_stmt, parse_while_stmt,
};
use crate::stream::TokenStream;
#[cfg(feature = "profiling")]
use std::time::Instant;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::TokenKind;

pub fn parse_stmt_or_decl_inner(s: &mut TokenStream) -> Result<StmtId, String> {
    while s.check(TokenKind::DocComment) {
        let lexeme: std::sync::Arc<str> = std::sync::Arc::from(s.lexeme());
        s.advance();
        s.store_pending_doc(lexeme);
    }

    let decorators = if s.check(TokenKind::At) {
        super::super::patterns::parse_decorator_list(s)?
    } else {
        Vec::new()
    };
    let kind = s.kind();
    let next_kind = s.peek_kind(1);

    let has_decorators = !decorators.is_empty();
    if let Some(decl_stmt) =
        super::super::stmt_decls::try_parse_decl_stmt(s, kind, next_kind, decorators)
    {
        return decl_stmt;
    }
    if has_decorators {
        return Err("expected a declaration after decorators".to_owned());
    }

    let _ = s.current_doc();

    match kind {
        TokenKind::LBrace => parse_block(s),
        TokenKind::Semicolon => {
            let range = s.range();
            s.advance();
            Ok(s.stmt(range, StmtKind::Empty))
        }
        TokenKind::If => parse_if_stmt(s),
        TokenKind::While => parse_while_stmt(s),
        TokenKind::Do => parse_do_while_stmt(s),
        TokenKind::For => parse_for_stmt(s),
        TokenKind::Switch => parse_switch_stmt(s),
        TokenKind::Return => parse_return_stmt(s),
        TokenKind::Break => parse_break_stmt(s),
        TokenKind::Continue => parse_continue_stmt(s),
        TokenKind::Throw => parse_throw_stmt(s),
        TokenKind::Try if s.peek_kind(1) == TokenKind::LBrace => parse_try_stmt(s),
        TokenKind::Using => parse_using_stmt(s, false),
        TokenKind::Await if next_kind == TokenKind::Using => parse_using_stmt(s, true),
        TokenKind::With => Err(String::from(
            "`with` is not supported; use explicit variable bindings",
        )),

        TokenKind::Identifier if next_kind == TokenKind::Colon => {
            let start_range = s.range();
            let label = s.consume_lexeme();
            s.advance();
            let body = parse_stmt_or_decl_inner(s)?;
            let range = s.span_from(start_range);
            Ok(s.stmt(range, StmtKind::Labeled { label, body }))
        }

        _ => {
            let start_range = s.range();
            let expr = crate::expressions::parse_seq_expr(s)?;
            s.eat_semicolon();
            let range = s.span_from(start_range);
            Ok(s.stmt(range, StmtKind::Expr { expression: expr }))
        }
    }
}

pub fn parse_block(s: &mut TokenStream) -> Result<StmtId, String> {
    #[cfg(feature = "profiling")]
    let started = Instant::now();
    let start_range = s.range();
    s.expect(TokenKind::LBrace)?;
    let mut stmts = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        while s.eat(TokenKind::Semicolon) {}
        if s.check(TokenKind::RBrace) {
            break;
        }
        match parse_stmt_or_decl_inner(s) {
            Ok(stmt) => stmts.push(stmt),
            Err(msg) => {
                let err_range = s.range();
                s.push_error(msg, err_range);
                loop {
                    match s.kind() {
                        TokenKind::EOF | TokenKind::RBrace => break,
                        TokenKind::Semicolon => {
                            s.advance();
                            break;
                        }
                        TokenKind::Return
                        | TokenKind::If
                        | TokenKind::For
                        | TokenKind::While
                        | TokenKind::Let
                        | TokenKind::Const
                        | TokenKind::Var
                        | TokenKind::Declare
                        | TokenKind::Function
                        | TokenKind::Class => break,
                        _ => {
                            s.advance();
                        }
                    }
                }
            }
        }
    }
    s.expect(TokenKind::RBrace)?;
    #[cfg(feature = "profiling")]
    {
        s.profile.block += started.elapsed();
    }
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::Block { stmts }))
}
