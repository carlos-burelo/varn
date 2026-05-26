use super::class::parse_class_decl;
use super::parse_function_decl;
use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use varn_core::ast::decl::{ExportDefaultDecl, ExportSpecifier, ImportSpecifier};
use varn_core::ast::{Decorator, ExportDecl, ImportDecl, StmtKind};
use varn_core::TokenKind;

pub fn parse_import_decl(s: &mut TokenStream) -> Result<ImportDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Import)?;

    let is_type = s.check(TokenKind::Type)
        && matches!(
            s.peek_kind(1),
            TokenKind::LBrace | TokenKind::Star | TokenKind::Identifier
        )
        && {
            s.advance();
            true
        };

    let mut specifiers = vec![];

    if s.check(TokenKind::Str) {
        let source = s.consume_lexeme();
        return Ok(ImportDecl {
            ast_id: 0,
            specifiers,
            source,
            is_type: false,
            range,
        });
    }

    if s.kind().can_be_identifier()
        && (s.peek_kind(1) == TokenKind::Comma
            || s.peek_kind(1) == TokenKind::From
            || s.peek_kind(1) == TokenKind::LBrace)
    {
        let local = s.consume_lexeme();
        specifiers.push(ImportSpecifier::Default {
            local,
            range: s.range(),
        });
        s.eat(TokenKind::Comma);
    }

    if s.eat(TokenKind::Star) {
        s.expect(TokenKind::As)?;
        let local = s.expect_lexeme(TokenKind::Identifier)?;
        specifiers.push(ImportSpecifier::Namespace {
            local,
            range: s.range(),
        });
    } else if s.check(TokenKind::LBrace) {
        s.advance();
        while !s.check(TokenKind::RBrace) && !s.is_eof() {
            let spec_range = s.range();
            let imported = s.consume_lexeme();
            let local = if s.eat(TokenKind::As) {
                s.consume_lexeme()
            } else {
                imported.clone()
            };
            specifiers.push(ImportSpecifier::Named {
                local,
                imported,
                range: spec_range,
            });
            if !s.eat(TokenKind::Comma) {
                break;
            }
        }
        s.expect(TokenKind::RBrace)?;
    }

    s.expect(TokenKind::From)?;
    let source = s.expect_lexeme(TokenKind::Str)?;

    Ok(ImportDecl {
        ast_id: 0,
        specifiers,
        source,
        is_type,
        range,
    })
}

pub fn parse_export_decl(
    s: &mut TokenStream,
    decorators: Vec<Decorator>,
) -> Result<ExportDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Export)?;
    let is_declare = s.eat(TokenKind::Declare);

    if s.check(TokenKind::Type) && s.peek_kind(1) == TokenKind::LBrace {
        s.advance();
        s.advance();
        while !s.check(TokenKind::RBrace) && !s.is_eof() {
            s.advance();
            if s.eat(TokenKind::As) {
                s.advance();
            }
            s.eat(TokenKind::Comma);
        }
        s.expect(TokenKind::RBrace)?;
        let source = if s.eat(TokenKind::From) {
            Some(s.expect_lexeme(TokenKind::Str)?)
        } else {
            None
        };
        s.eat_semicolon();
        return Ok(ExportDecl::Named {
            ast_id: 0,
            specifiers: vec![],
            source,
            range,
        });
    }

    if s.eat(TokenKind::Default) {
        let decl = match s.kind() {
            TokenKind::Function | TokenKind::Async => {
                let is_async = s.eat(TokenKind::Async);
                let mut fn_decl = parse_function_decl(s, decorators.clone(), is_async, is_declare)?;
                fn_decl.doc = s.take_pending_doc().map(|doc| doc.to_string());
                ExportDefaultDecl::Function(fn_decl)
            }
            TokenKind::Class | TokenKind::Abstract => {
                let mut cls = parse_class_decl(s, decorators.clone(), is_declare)?;
                cls.doc = s.take_pending_doc().map(|doc| doc.to_string());
                ExportDefaultDecl::Class(cls)
            }
            _ => {
                let expr = parse_expr(s)?;
                s.eat_semicolon();
                ExportDefaultDecl::Expr(expr)
            }
        };
        return Ok(ExportDecl::Default {
            ast_id: 0,
            declaration: Box::new(decl),
            range,
        });
    }

    if s.eat(TokenKind::Star) {
        let alias = if s.eat(TokenKind::As) {
            Some(s.consume_lexeme())
        } else {
            None
        };
        s.expect(TokenKind::From)?;
        let source = s.expect_lexeme(TokenKind::Str)?;
        return Ok(ExportDecl::All {
            ast_id: 0,
            source,
            alias,
            range,
        });
    }

    if s.check(TokenKind::LBrace) {
        s.advance();
        let mut specifiers = vec![];
        while !s.check(TokenKind::RBrace) && !s.is_eof() {
            let spec_range = s.range();
            let local = s.consume_lexeme();
            let exported = if s.eat(TokenKind::As) {
                s.consume_lexeme()
            } else {
                local.clone()
            };
            specifiers.push(ExportSpecifier {
                local,
                exported,
                range: spec_range,
            });
            if !s.eat(TokenKind::Comma) {
                break;
            }
        }
        s.expect(TokenKind::RBrace)?;
        let source = if s.eat(TokenKind::From) {
            Some(s.expect_lexeme(TokenKind::Str)?)
        } else {
            None
        };
        return Ok(ExportDecl::Named {
            ast_id: 0,
            specifiers,
            source,
            range,
        });
    }

    let decl = if is_declare {
        match super::super::stmt_decls::try_parse_decl_stmt_mode(
            s,
            s.kind(),
            s.peek_kind(1),
            decorators,
            true,
        ) {
            Some(Ok(stmt)) => stmt,
            Some(Err(e)) => return Err(e),
            None => return Err("Expected declaration after `export declare`".to_owned()),
        }
    } else {
        match super::super::stmt_decls::try_parse_decl_stmt_mode(
            s,
            s.kind(),
            s.peek_kind(1),
            decorators,
            false,
        ) {
            Some(Ok(stmt)) => stmt,
            Some(Err(e)) => return Err(e),
            None => super::super::stmts::parse_stmt_or_decl_inner(s)?,
        }
    };
    if let StmtKind::Decl(d) = decl.kind {
        return Ok(ExportDecl::Decl {
            ast_id: 0,
            declaration: d,
            range,
        });
    }

    Err("Expected declaration after `export`".to_owned())
}
