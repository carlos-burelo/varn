use super::class::parse_class_decl;
use super::parse_function_decl;
use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use varn_core::ast::decl::{ExportDefaultDecl, ExportSpecifier};
use varn_core::ast::{Decorator, ExportDecl, StmtKind};
use varn_core::TokenKind;

pub fn parse_export_decl(
    s: &mut TokenStream,
    decorators: Vec<Decorator>,
) -> Result<ExportDecl, String> {
    let range = s.range();
    s.expect(TokenKind::Export)?;
    let is_declare = s.eat(TokenKind::Declare);

    if s.check(TokenKind::Type) && s.peek_kind(1) == TokenKind::LBrace {
        if !decorators.is_empty() {
            return Err("decorators are not supported on type exports".to_owned());
        }
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
            let text = s.consume_str();
            Some(s.interner.intern(&text))
        } else {
            None
        };
        s.eat_semicolon();
        let full_range = s.span_from(range);
        return Ok(ExportDecl::Named {
            ast_id: s.next_ast_id(),
            specifiers: vec![],
            source,
            range: full_range,
        });
    }

    if s.eat(TokenKind::Default) {
        let decl = match s.kind() {
            TokenKind::Function | TokenKind::Async => {
                let is_async = s.eat(TokenKind::Async);
                let mut fn_decl = parse_function_decl(s, decorators.clone(), is_async, is_declare)?;
                fn_decl.doc = s.current_doc();
                ExportDefaultDecl::Function(fn_decl)
            }
            TokenKind::Class | TokenKind::Abstract => {
                let mut cls = parse_class_decl(s, decorators.clone(), is_declare)?;
                cls.doc = s.current_doc();
                ExportDefaultDecl::Class(cls)
            }
            TokenKind::EOF
            | TokenKind::Dynamic
            | TokenKind::Identifier
            | TokenKind::IntegerLiteral
            | TokenKind::FloatLiteral
            | TokenKind::BinaryLiteral
            | TokenKind::OctalLiteral
            | TokenKind::HexLiteral
            | TokenKind::BigIntLiteral
            | TokenKind::Str
            | TokenKind::Char
            | TokenKind::Template
            | TokenKind::TemplateHead
            | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail
            | TokenKind::RegularExpression
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LBracket
            | TokenKind::RBracket
            | TokenKind::LAngle
            | TokenKind::RAngle
            | TokenKind::Semicolon
            | TokenKind::Comma
            | TokenKind::Dot
            | TokenKind::DotDot
            | TokenKind::DotDotDot
            | TokenKind::DotDotEq
            | TokenKind::Colon
            | TokenKind::ColonColon
            | TokenKind::Question
            | TokenKind::QuestionDot
            | TokenKind::QuestionLBracket
            | TokenKind::QuestionQuestion
            | TokenKind::QuestionQuestionEq
            | TokenKind::Plus
            | TokenKind::PlusPlus
            | TokenKind::PlusEq
            | TokenKind::Minus
            | TokenKind::MinusMinus
            | TokenKind::MinusEq
            | TokenKind::Star
            | TokenKind::StarStar
            | TokenKind::StarEq
            | TokenKind::StarStarEq
            | TokenKind::Slash
            | TokenKind::SlashEq
            | TokenKind::Percent
            | TokenKind::PercentEq
            | TokenKind::Amp
            | TokenKind::AmpAmp
            | TokenKind::AmpEq
            | TokenKind::AmpAmpEq
            | TokenKind::Pipe
            | TokenKind::PipePipe
            | TokenKind::PipeEq
            | TokenKind::PipePipeEq
            | TokenKind::PipeGt
            | TokenKind::Caret
            | TokenKind::CaretEq
            | TokenKind::Tilde
            | TokenKind::LtLt
            | TokenKind::LtLtEq
            | TokenKind::GtGt
            | TokenKind::GtGtEq
            | TokenKind::GtGtGt
            | TokenKind::GtGtGtEq
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::EqEqEq
            | TokenKind::Bang
            | TokenKind::BangEq
            | TokenKind::BangEqEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::Arrow
            | TokenKind::FatArrow
            | TokenKind::Let
            | TokenKind::Const
            | TokenKind::Var
            | TokenKind::Struct
            | TokenKind::Interface
            | TokenKind::Type
            | TokenKind::Enum
            | TokenKind::Namespace
            | TokenKind::Module
            | TokenKind::Extension
            | TokenKind::On
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::Switch
            | TokenKind::Case
            | TokenKind::Default
            | TokenKind::While
            | TokenKind::For
            | TokenKind::Do
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
            | TokenKind::Throw
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Using
            | TokenKind::With
            | TokenKind::Import
            | TokenKind::Export
            | TokenKind::From
            | TokenKind::As
            | TokenKind::Await
            | TokenKind::Yield
            | TokenKind::New
            | TokenKind::This
            | TokenKind::Super
            | TokenKind::Delete
            | TokenKind::Typeof
            | TokenKind::Instanceof
            | TokenKind::In
            | TokenKind::Of
            | TokenKind::Void
            | TokenKind::Is
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::Public
            | TokenKind::Private
            | TokenKind::Protected
            | TokenKind::Static
            | TokenKind::Override
            | TokenKind::Readonly
            | TokenKind::Declare
            | TokenKind::Native
            | TokenKind::Extends
            | TokenKind::Implements
            | TokenKind::Get
            | TokenKind::Set
            | TokenKind::Constructor
            | TokenKind::Destructor
            | TokenKind::Match
            | TokenKind::At
            | TokenKind::Hash
            | TokenKind::Backslash
            | TokenKind::Dollar
            | TokenKind::Backtick
            | TokenKind::Newline
            | TokenKind::Whitespace
            | TokenKind::DocComment
            | TokenKind::Placeholder
            | TokenKind::DecimalLiteral
            | TokenKind::Spawn
            | TokenKind::Parallel
            | TokenKind::Start
            | TokenKind::RawStr => {
                if !decorators.is_empty() {
                    return Err(
                        "decorators are not supported on default export expressions".to_owned()
                    );
                }
                let expr = parse_expr(s)?;
                s.eat_semicolon();
                ExportDefaultDecl::Expr(expr)
            }
        };
        let full_range = s.span_from(range);
        return Ok(ExportDecl::Default {
            ast_id: s.next_ast_id(),
            declaration: Box::new(decl),
            range: full_range,
        });
    }

    if s.eat(TokenKind::Star) {
        if !decorators.is_empty() {
            return Err("decorators are not supported on export all".to_owned());
        }
        let alias = if s.eat(TokenKind::As) {
            Some(s.consume_lexeme())
        } else {
            None
        };
        s.expect(TokenKind::From)?;
        let source = s.consume_str();
        let source = s.interner.intern(&source);
        let full_range = s.span_from(range);
        return Ok(ExportDecl::All {
            ast_id: s.next_ast_id(),
            source,
            alias,
            range: full_range,
        });
    }

    if s.check(TokenKind::LBrace) {
        if !decorators.is_empty() {
            return Err("decorators are not supported on named exports".to_owned());
        }
        s.advance();
        let mut specifiers = vec![];
        while !s.check(TokenKind::RBrace) && !s.is_eof() {
            let spec_range = s.range();
            let local = s.consume_lexeme();
            let exported = if s.eat(TokenKind::As) {
                s.consume_lexeme()
            } else {
                local
            };
            let full_spec_range = s.span_from(spec_range);
            specifiers.push(ExportSpecifier {
                local,
                exported,
                range: full_spec_range,
            });
            if !s.eat(TokenKind::Comma) {
                break;
            }
        }
        s.expect(TokenKind::RBrace)?;
        let source = if s.eat(TokenKind::From) {
            let text = s.consume_str();
            Some(s.interner.intern(&text))
        } else {
            None
        };
        let full_range = s.span_from(range);
        return Ok(ExportDecl::Named {
            ast_id: s.next_ast_id(),
            specifiers,
            source,
            range: full_range,
        });
    }

    let has_outer_decorators = !decorators.is_empty();
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
            None => {
                if has_outer_decorators {
                    return Err("expected a declaration after decorators".to_owned());
                }
                super::super::stmts::parse_stmt_or_decl_inner(s)?
            }
        }
    };
    if let StmtKind::Decl(d) = s.arena.stmt(decl).kind.clone() {
        let full_range = s.span_from(range);
        return Ok(ExportDecl::Decl {
            ast_id: s.next_ast_id(),
            declaration: d,
            range: full_range,
        });
    }

    Err("Expected declaration after `export`".to_owned())
}
