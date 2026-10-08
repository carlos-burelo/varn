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
            | TokenKind::Function
            | TokenKind::Class
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
            | TokenKind::Async
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
            | TokenKind::Abstract
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
            | TokenKind::RawStr => return Err(String::from("Expected `let` or `const`")),
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
