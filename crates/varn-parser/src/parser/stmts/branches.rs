use super::dispatch::parse_stmt_or_decl_inner;
use crate::stream::TokenStream;
use varn_core::ast::operators::VarKind;
use varn_core::ast::{ForInit, StmtId, StmtKind, SwitchCase};
use varn_core::TokenKind;

pub(super) fn parse_if_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    let consequent = parse_stmt_or_decl_inner(s)?;
    let alternate = if s.eat(TokenKind::Else) {
        Some(parse_stmt_or_decl_inner(s)?)
    } else {
        None
    };
    let range = s.span_from(start_range);
    Ok(s.stmt(
        range,
        StmtKind::If {
            test,
            consequent,
            alternate,
        },
    ))
}

pub(super) fn parse_while_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    let body = parse_stmt_or_decl_inner(s)?;
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::While { test, body }))
}

pub(super) fn parse_do_while_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    let body = parse_stmt_or_decl_inner(s)?;
    s.expect(TokenKind::While)?;
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    s.eat_semicolon();
    let range = s.span_from(start_range);
    Ok(s.stmt(range, StmtKind::DoWhile { body, test }))
}

pub(super) fn parse_for_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    let is_await = s.eat(TokenKind::Await);
    s.expect(TokenKind::LParen)?;

    let is_var_decl_head = matches!(s.kind(), TokenKind::Let | TokenKind::Const | TokenKind::Var);

    let init = if is_var_decl_head {
        let kind = match s.kind() {
            TokenKind::Let => VarKind::Let,
            TokenKind::Const => VarKind::Const,
            TokenKind::Var => {
                let err_range = s.range();
                s.push_error(
                    "`var` is not supported; use `let` or `const`".to_owned(),
                    err_range,
                );
                VarKind::Let
            }
            _ => return Err(String::from("Expected `let` or `const`")),
        };
        let decl_range = s.range();
        s.advance();
        let head_range = s.range();
        let pat = super::super::patterns::parse_pattern(s)?;

        if s.eat(TokenKind::In) {
            let right = crate::expressions::parse_seq_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let body = parse_stmt_or_decl_inner(s)?;
            let range = s.span_from(start_range);
            return Ok(s.stmt(
                range,
                StmtKind::ForIn {
                    kind,
                    left: pat,
                    right,
                    body,
                },
            ));
        }
        if s.eat(TokenKind::Of) {
            let right = crate::expressions::parse_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let body = parse_stmt_or_decl_inner(s)?;
            let range = s.span_from(start_range);
            return Ok(s.stmt(
                range,
                StmtKind::ForOf {
                    kind,
                    left: pat,
                    right,
                    body,
                    is_await,
                },
            ));
        }

        let decl = super::super::decls::parse_var_decl_after_head(
            s, decl_range, kind, head_range, pat, false,
        )?;
        Some(Box::new(ForInit::Var {
            kind: decl.kind,
            declarators: decl.declarators,
        }))
    } else if s.check(TokenKind::Semicolon) {
        None
    } else {
        let expr = crate::expressions::parse_seq_expr(s)?;
        Some(Box::new(ForInit::Expr(expr)))
    };

    s.expect(TokenKind::Semicolon)?;
    let test = if s.check(TokenKind::Semicolon) {
        None
    } else {
        Some(crate::expressions::parse_seq_expr(s)?)
    };
    s.expect(TokenKind::Semicolon)?;
    let update = if s.check(TokenKind::RParen) {
        None
    } else {
        Some(crate::expressions::parse_seq_expr(s)?)
    };
    s.expect(TokenKind::RParen)?;
    let body = parse_stmt_or_decl_inner(s)?;

    let range = s.span_from(start_range);
    Ok(s.stmt(
        range,
        StmtKind::For {
            init,
            test,
            update,
            body,
        },
    ))
}

pub(super) fn parse_switch_stmt(s: &mut TokenStream) -> Result<StmtId, String> {
    let start_range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let discriminant = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    s.expect(TokenKind::LBrace)?;

    let mut cases = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        let case_start = s.range();
        let test = if s.eat(TokenKind::Case) {
            Some(crate::expressions::parse_seq_expr(s)?)
        } else {
            s.expect(TokenKind::Default)?;
            None
        };
        s.expect(TokenKind::Colon)?;

        let mut body = vec![];
        while !matches!(
            s.kind(),
            TokenKind::Case | TokenKind::Default | TokenKind::RBrace | TokenKind::EOF
        ) {
            body.push(parse_stmt_or_decl_inner(s)?);
        }
        let case_range = s.span_from(case_start);
        cases.push(SwitchCase {
            test,
            body,
            range: case_range,
        });
    }
    s.expect(TokenKind::RBrace)?;
    let range = s.span_from(start_range);
    Ok(s.stmt(
        range,
        StmtKind::Switch {
            discriminant,
            cases,
        },
    ))
}
