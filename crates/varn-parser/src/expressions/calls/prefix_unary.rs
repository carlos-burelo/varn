use super::postfix_update::parse_postfix_expr;
use crate::stream::TokenStream;
use varn_core::ast::operators::{UnaryOp, UpdateOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

pub fn parse_unary_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start_range = s.range();

    macro_rules! prefix_unary {
        ($op:expr) => {{
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Unary {
                    op: $op,
                    prefix: true,
                    operand: o,
                },
            ))
        }};
    }

    match s.kind() {
        TokenKind::Bang => prefix_unary!(UnaryOp::Not),
        TokenKind::Tilde => prefix_unary!(UnaryOp::BitNot),
        TokenKind::Minus => {
            const I64_MIN_MAGNITUDE: &str = "9223372036854775808";
            s.advance();
            if s.kind() == TokenKind::IntegerLiteral
                && s.lexeme().replace('_', "") == I64_MIN_MAGNITUDE
            {
                s.advance();
                let full_range = s.span_from(start_range);
                let raw = s.interner.intern(&format!("-{}", I64_MIN_MAGNITUDE));
                return Ok(s.expr(
                    full_range,
                    ExprKind::IntLiteral {
                        value: i64::MIN,
                        raw,
                    },
                ));
            }

            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Unary {
                    op: UnaryOp::Minus,
                    prefix: true,
                    operand: o,
                },
            ))
        }
        TokenKind::Plus => prefix_unary!(UnaryOp::Plus),
        TokenKind::Typeof => prefix_unary!(UnaryOp::Typeof),
        TokenKind::Void => Err("`void` is not supported; use `_` to discard values".to_owned()),
        TokenKind::Delete => Err("`delete` is not supported".to_owned()),
        TokenKind::Await => {
            s.advance();
            let argument = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(full_range, ExprKind::Await { argument }))
        }
        TokenKind::Try => {
            s.advance();
            let operand = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Try {
                    expression: operand,
                },
            ))
        }
        TokenKind::PlusPlus => {
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Update {
                    op: UpdateOp::Increment,
                    prefix: true,
                    operand: o,
                },
            ))
        }
        TokenKind::MinusMinus => {
            s.advance();
            let o = parse_unary_expr(s)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(
                full_range,
                ExprKind::Update {
                    op: UpdateOp::Decrement,
                    prefix: true,
                    operand: o,
                },
            ))
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
        | TokenKind::PlusEq
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
        | TokenKind::LtLt
        | TokenKind::LtLtEq
        | TokenKind::GtGt
        | TokenKind::GtGtEq
        | TokenKind::GtGtGt
        | TokenKind::GtGtGtEq
        | TokenKind::Eq
        | TokenKind::EqEq
        | TokenKind::EqEqEq
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
        | TokenKind::Catch
        | TokenKind::Finally
        | TokenKind::Using
        | TokenKind::With
        | TokenKind::Import
        | TokenKind::Export
        | TokenKind::From
        | TokenKind::As
        | TokenKind::Async
        | TokenKind::Yield
        | TokenKind::New
        | TokenKind::This
        | TokenKind::Super
        | TokenKind::Instanceof
        | TokenKind::In
        | TokenKind::Of
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
        | TokenKind::RawStr => parse_postfix_expr(s),
    }
}
