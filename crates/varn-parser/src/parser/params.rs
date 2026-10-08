use super::patterns::parse_pattern;
use crate::expressions::parse_expr;
use crate::stream::TokenStream;
use crate::types::parse_type;
use varn_core::ast::operators::{Modifiers, Visibility};
use varn_core::ast::Param;
use varn_core::TokenKind;

pub fn parse_params(s: &mut TokenStream) -> Result<Vec<Param>, String> {
    s.expect(TokenKind::LParen)?;
    let mut params = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        params.push(parse_param(s)?);
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    s.expect(TokenKind::RParen)?;
    Ok(params)
}

pub fn parse_single_param(s: &mut TokenStream) -> Result<Param, String> {
    parse_param(s)
}

fn parse_param(s: &mut TokenStream) -> Result<Param, String> {
    let range = s.range();
    let mut mods = Modifiers::default();

    loop {
        match s.kind() {
            TokenKind::Public => {
                mods.visibility = Some(Visibility::Public);
                s.advance();
            }
            TokenKind::Private => {
                mods.visibility = Some(Visibility::Private);
                s.advance();
            }
            TokenKind::Protected => {
                mods.visibility = Some(Visibility::Protected);
                s.advance();
            }
            TokenKind::Readonly => {
                mods.is_readonly = true;
                s.advance();
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
            | TokenKind::Static
            | TokenKind::Abstract
            | TokenKind::Override
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
            | TokenKind::RawStr => break,
        }
    }

    let is_rest = s.eat(TokenKind::DotDotDot);
    let pattern = parse_pattern(s)?;
    let is_optional = s.eat(TokenKind::Question);
    let type_ann = if s.eat(TokenKind::Colon) {
        Some(parse_type(s)?)
    } else {
        None
    };
    let default = if s.eat(TokenKind::Eq) {
        Some(parse_expr(s)?)
    } else {
        None
    };

    let full_range = s.span_from(range);
    Ok(Param {
        pattern,
        type_ann,
        default,
        is_rest,
        is_optional,
        modifiers: mods,
        range: full_range,
    })
}
