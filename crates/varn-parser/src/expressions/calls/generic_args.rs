use super::super::parse_assign_expr;
use crate::stream::TokenStream;
use crate::types::parse_type_args;
use varn_core::ast::expr::Arg;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::SourceRange;
use varn_core::TokenKind;

pub(super) fn looks_like_generic_call(s: &TokenStream) -> bool {
    let mut depth = 0i32;
    let mut off = 0usize;
    loop {
        match s.peek_kind(off) {
            TokenKind::LAngle => depth += 1,
            TokenKind::RAngle => {
                depth -= 1;
                if depth == 0 {
                    return s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGt => {
                depth -= 2;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::GtGtGt => {
                depth -= 3;
                if depth <= 0 {
                    return depth == 0 && s.peek_kind(off + 1) == TokenKind::LParen;
                }
            }
            TokenKind::EOF
            | TokenKind::Semicolon
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::Eq
            | TokenKind::FatArrow => return false,
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
            | TokenKind::LBracket
            | TokenKind::RBracket
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
            | TokenKind::GtGtEq
            | TokenKind::GtGtGtEq
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
            | TokenKind::RawStr => {}
        }
        off += 1;
    }
}

pub(super) fn try_parse_generic_call(
    s: &mut TokenStream,
    callee: ExprId,
    expr_range: SourceRange,
) -> Result<ExprId, String> {
    let type_args = parse_type_args(s)?;
    if !s.check(TokenKind::LParen) {
        return Err("not a generic call".to_owned());
    }
    let (_, args, call_range) = parse_call_args(s)?;
    Ok(s.expr(
        expr_range.to(call_range),
        ExprKind::Call {
            callee,
            type_args,
            args,
            optional: false,
        },
    ))
}

pub fn parse_call_args(
    s: &mut TokenStream,
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
    s.expect(TokenKind::LParen)?;
    let mut args = vec![];
    while !s.check(TokenKind::RParen) && !s.is_eof() {
        if s.check(TokenKind::DotDotDot) {
            s.advance();
            args.push(Arg::Spread(parse_assign_expr(s)?));
        } else if s.check(TokenKind::Identifier) && s.peek_kind(1) == TokenKind::Colon {
            let label = s.consume_lexeme();
            s.advance();
            let label = s.interner.resolve(label).to_string();
            args.push(Arg::Named {
                label,
                value: parse_assign_expr(s)?,
            });
        } else {
            args.push(Arg::Positional(parse_assign_expr(s)?));
        }
        if !s.eat(TokenKind::Comma) {
            break;
        }
    }
    let rparen = s.expect_token(TokenKind::RParen)?;
    Ok((vec![], args, rparen.range))
}

pub fn parse_call_args_pub(
    s: &mut TokenStream,
) -> Result<(Vec<varn_core::ast::TypeNode>, Vec<Arg>, SourceRange), String> {
    parse_call_args(s)
}
