use crate::stream::TokenStream;
#[cfg(feature = "profiling")]
use std::time::Instant;
use varn_core::ast::operators::VarKind;
use varn_core::ast::{CatchClause, ForInit, Stmt, StmtKind, SwitchCase, VarDeclarator};
use varn_core::TokenKind;

pub fn parse_stmt_or_decl_inner(s: &mut TokenStream) -> Result<Stmt, String> {
    if s.check(TokenKind::DocComment) {
        let lexeme = s.consume_lexeme();
        s.store_pending_doc(lexeme);
    }

    let decorators = if s.check(TokenKind::At) {
        super::patterns::parse_decorator_list(s)?
    } else {
        Vec::new()
    };
    let kind = s.kind();
    let next_kind = s.peek_kind(1);

    if let Some(decl_stmt) = super::stmt_decls::try_parse_decl_stmt(s, kind, next_kind, decorators)
    {
        return decl_stmt;
    }

    // Doc comment before a non-declaration statement — discard to avoid leaking.
    let _ = s.take_pending_doc();

    match kind {
        TokenKind::LBrace => parse_block(s),
        TokenKind::Semicolon => {
            let range = s.range();
            s.advance();
            Ok(Stmt::new_with_range(range, StmtKind::Empty))
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
        TokenKind::Try => parse_try_stmt(s),
        TokenKind::Using => parse_using_stmt(s, false),
        TokenKind::Await if next_kind == TokenKind::Using => parse_using_stmt(s, true),
        TokenKind::With => Err(String::from(
            "`with` is not supported; use explicit variable bindings",
        )),

        TokenKind::Identifier if next_kind == TokenKind::Colon => {
            let range = s.range();
            let label = s.consume_lexeme();
            s.advance();
            let body = parse_stmt_or_decl_inner(s)?;
            Ok(Stmt::new_with_range(
                range,
                StmtKind::Labeled {
                    label,
                    body: Box::new(body),
                },
            ))
        }

        _ => {
            let range = s.range();
            let expr = crate::expressions::parse_seq_expr(s)?;
            s.eat_semicolon();
            Ok(Stmt::new_with_range(
                range,
                StmtKind::Expr {
                    expression: Box::new(expr),
                },
            ))
        }
    }
}

pub fn parse_block(s: &mut TokenStream) -> Result<Stmt, String> {
    #[cfg(feature = "profiling")]
    let started = Instant::now();
    let range = s.range();
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
    Ok(Stmt::new_with_range(range, StmtKind::Block { stmts }))
}

fn parse_if_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    let consequent = parse_stmt_or_decl_inner(s)?;
    let alternate = if s.eat(TokenKind::Else) {
        Some(Box::new(parse_stmt_or_decl_inner(s)?))
    } else {
        None
    };
    Ok(Stmt::new_with_range(
        range,
        StmtKind::If {
            test: Box::new(test),
            consequent: Box::new(consequent),
            alternate,
        },
    ))
}

fn parse_while_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    let body = parse_stmt_or_decl_inner(s)?;
    Ok(Stmt::new_with_range(
        range,
        StmtKind::While {
            test: Box::new(test),
            body: Box::new(body),
        },
    ))
}

fn parse_do_while_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    let body = parse_stmt_or_decl_inner(s)?;
    s.expect(TokenKind::While)?;
    s.expect(TokenKind::LParen)?;
    let test = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    s.eat_semicolon();
    Ok(Stmt::new_with_range(
        range,
        StmtKind::DoWhile {
            body: Box::new(body),
            test: Box::new(test),
        },
    ))
}

fn parse_for_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    let is_await = s.eat(TokenKind::Await);
    s.expect(TokenKind::LParen)?;

    let is_var_decl_head = matches!(s.kind(), TokenKind::Let | TokenKind::Const | TokenKind::Var);

    let init = if is_var_decl_head {
        let kind = match s.kind() {
            TokenKind::Let => VarKind::Let,
            TokenKind::Const => VarKind::Const,
            _ => return Err(String::from("`var` is not supported; use `let` or `const`")),
        };
        let decl_range = s.range();
        s.advance();
        let head_range = s.range();
        let pat = super::patterns::parse_pattern(s)?;

        if s.eat(TokenKind::In) {
            let right = crate::expressions::parse_seq_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let body = parse_stmt_or_decl_inner(s)?;
            return Ok(Stmt::new_with_range(
                range,
                StmtKind::ForIn {
                    kind,
                    left: pat,
                    right: Box::new(right),
                    body: Box::new(body),
                },
            ));
        }
        if s.eat(TokenKind::Of) {
            let right = crate::expressions::parse_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let body = parse_stmt_or_decl_inner(s)?;
            return Ok(Stmt::new_with_range(
                range,
                StmtKind::ForOf {
                    kind,
                    left: pat,
                    right: Box::new(right),
                    body: Box::new(body),
                    is_await,
                },
            ));
        }

        let decl =
            super::decls::parse_var_decl_after_head(s, decl_range, kind, head_range, pat, false)?;
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
        Some(Box::new(crate::expressions::parse_seq_expr(s)?))
    };
    s.expect(TokenKind::Semicolon)?;
    let update = if s.check(TokenKind::RParen) {
        None
    } else {
        Some(Box::new(crate::expressions::parse_seq_expr(s)?))
    };
    s.expect(TokenKind::RParen)?;
    let body = parse_stmt_or_decl_inner(s)?;

    Ok(Stmt::new_with_range(
        range,
        StmtKind::For {
            init,
            test,
            update,
            body: Box::new(body),
        },
    ))
}

fn parse_switch_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    s.expect(TokenKind::LParen)?;
    let discriminant = crate::expressions::parse_seq_expr(s)?;
    s.expect(TokenKind::RParen)?;
    s.expect(TokenKind::LBrace)?;

    let mut cases = vec![];
    while !s.check(TokenKind::RBrace) && !s.is_eof() {
        let case_range = s.range();
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
        cases.push(SwitchCase {
            test,
            body,
            range: case_range,
        });
    }
    s.expect(TokenKind::RBrace)?;
    Ok(Stmt::new_with_range(
        range,
        StmtKind::Switch {
            discriminant: Box::new(discriminant),
            cases,
        },
    ))
}

fn parse_using_stmt(s: &mut TokenStream, is_await: bool) -> Result<Stmt, String> {
    let range = s.range();
    if is_await {
        s.expect(TokenKind::Await)?;
    }
    s.expect(TokenKind::Using)?;

    let mut declarations = Vec::new();
    loop {
        let decl_range = s.range();
        let id = super::patterns::parse_pattern(s)?;
        let type_ann = if s.eat(TokenKind::Colon) {
            Some(crate::types::parse_type(s)?)
        } else {
            None
        };
        s.expect(TokenKind::Eq)?;
        let init = Some(crate::expressions::parse_seq_expr(s)?);

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

    Ok(Stmt::new_with_range(
        range,
        StmtKind::Using {
            declarations,
            is_await,
        },
    ))
}

fn parse_return_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    let argument = if !s.check(TokenKind::Semicolon) && !s.check(TokenKind::RBrace) && !s.is_eof() {
        Some(Box::new(crate::expressions::parse_seq_expr(s)?))
    } else {
        None
    };
    s.eat_semicolon();
    Ok(Stmt::new_with_range(range, StmtKind::Return { argument }))
}

fn parse_break_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();

    let label = if s.check(TokenKind::Identifier)
        && !s.check(TokenKind::Semicolon)
        && s.line() == s.prev_line()
    {
        Some(s.consume_lexeme())
    } else {
        None
    };
    s.eat_semicolon();
    Ok(Stmt::new_with_range(range, StmtKind::Break { label }))
}

fn parse_continue_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();

    let label = if s.check(TokenKind::Identifier)
        && !s.check(TokenKind::Semicolon)
        && s.line() == s.prev_line()
    {
        Some(s.consume_lexeme())
    } else {
        None
    };
    s.eat_semicolon();
    Ok(Stmt::new_with_range(range, StmtKind::Continue { label }))
}

fn parse_throw_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    let argument = crate::expressions::parse_seq_expr(s)?;
    s.eat_semicolon();
    Ok(Stmt::new_with_range(
        range,
        StmtKind::Throw {
            argument: Box::new(argument),
        },
    ))
}

fn parse_try_stmt(s: &mut TokenStream) -> Result<Stmt, String> {
    let range = s.range();
    s.advance();
    let block = parse_block(s)?;

    let catch = if s.eat(TokenKind::Catch) {
        let param = if s.eat(TokenKind::LParen) {
            let p = Some(super::patterns::parse_pattern(s)?);
            s.expect(TokenKind::RParen)?;
            p
        } else {
            None
        };
        let body = parse_block(s)?;
        Some(Box::new(CatchClause {
            param,
            body: Box::new(body),
            range: s.range(),
        }))
    } else {
        None
    };

    let finally = if s.eat(TokenKind::Finally) {
        Some(Box::new(parse_block(s)?))
    } else {
        None
    };

    Ok(Stmt::new_with_range(
        range,
        StmtKind::Try {
            block: Box::new(block),
            catch,
            finally,
        },
    ))
}
