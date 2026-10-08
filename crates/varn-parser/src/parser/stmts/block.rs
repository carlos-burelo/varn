use super::dispatch::parse_stmt_or_decl_inner;
use crate::stream::TokenStream;
#[cfg(feature = "profiling")]
use std::time::Instant;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::TokenKind;

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
                        TokenKind::Dynamic
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
                        | TokenKind::LBracket
                        | TokenKind::RBracket
                        | TokenKind::LAngle
                        | TokenKind::RAngle
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
                        | TokenKind::Struct
                        | TokenKind::Interface
                        | TokenKind::Type
                        | TokenKind::Enum
                        | TokenKind::Namespace
                        | TokenKind::Module
                        | TokenKind::Extension
                        | TokenKind::On
                        | TokenKind::Else
                        | TokenKind::Switch
                        | TokenKind::Case
                        | TokenKind::Default
                        | TokenKind::Do
                        | TokenKind::Break
                        | TokenKind::Continue
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
