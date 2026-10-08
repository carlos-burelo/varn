mod array_expr;
mod class_expr;
mod function_expr;
mod match_expr;
mod new_expr;
mod object;
mod template;

use super::literal_text::{parse_int_radix, split_regex, unescape_string};
use super::{parse_seq_expr, try_parse_arrow};
use crate::stream::TokenStream;
use array_expr::parse_array_expr;
use class_expr::parse_class_expr;
use function_expr::{parse_function_expr, parse_function_expr_inner_with_start};
use new_expr::parse_new_expr;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::ParsedNumber;
use varn_core::TokenKind;

use self::match_expr::parse_match_expr;
pub(crate) use self::object::parse_object_body;
use self::object::parse_object_expr;
pub(crate) use self::template::parse_template;

pub fn parse_primary_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let range = s.range();

    match s.kind() {
        TokenKind::IntegerLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral
        | TokenKind::HexLiteral => {
            let pre_parsed = s.parsed_num();
            let raw = s.consume_lexeme();
            let raw_text = s.interner.resolve(raw);
            let value: i64 = match pre_parsed {
                Some(ParsedNumber::Int(v)) => v,
                Some(ParsedNumber::Float(_)) | None => parse_int_radix(raw_text)
                    .ok_or_else(|| format!("integer literal `{}` overflows i64", raw_text))?,
            };
            Ok(s.expr(range, ExprKind::IntLiteral { value, raw }))
        }
        TokenKind::FloatLiteral => {
            let pre_parsed = s.parsed_num();
            let raw = s.consume_lexeme();
            let raw_text = s.interner.resolve(raw);
            let value: f64 = match pre_parsed {
                Some(ParsedNumber::Float(v)) => v,
                Some(ParsedNumber::Int(_)) | None => raw_text
                    .parse()
                    .map_err(|_| format!("invalid float literal: {}", raw_text))?,
            };
            Ok(s.expr(range, ExprKind::FloatLiteral { value, raw }))
        }
        TokenKind::BigIntLiteral => {
            let raw = s.consume_lexeme();
            Ok(s.expr(range, ExprKind::BigIntLiteral { raw }))
        }
        TokenKind::DecimalLiteral => {
            let raw = s.consume_lexeme();
            Ok(s.expr(range, ExprKind::DecimalLiteral { raw }))
        }
        TokenKind::RawStr => {
            let atom = s.consume_lexeme();
            let value = s.interner.resolve(atom).to_string();
            Ok(s.expr(range, ExprKind::StrLiteral { value }))
        }
        TokenKind::Str => {
            let value = unescape_string(s.lexeme());
            s.advance();
            Ok(s.expr(range, ExprKind::StrLiteral { value }))
        }
        TokenKind::Char => {
            let ch = unescape_string(s.lexeme()).chars().next().unwrap_or('\0');
            s.advance();
            Ok(s.expr(range, ExprKind::CharLiteral { value: ch }))
        }
        TokenKind::True => {
            s.advance();
            Ok(s.expr(range, ExprKind::BoolLiteral { value: true }))
        }
        TokenKind::False => {
            s.advance();
            Ok(s.expr(range, ExprKind::BoolLiteral { value: false }))
        }
        TokenKind::Null => {
            s.advance();
            Ok(s.expr(range, ExprKind::NullLiteral))
        }
        TokenKind::RegularExpression => {
            let raw = s.consume_lexeme();
            let (pattern, flags) = split_regex(s.interner.resolve(raw));
            Ok(s.expr(range, ExprKind::RegexLiteral { pattern, flags }))
        }

        TokenKind::Template | TokenKind::TemplateHead => parse_template(s),

        TokenKind::Identifier => {
            let name = s.consume_lexeme();
            Ok(s.expr(range, ExprKind::Identifier { name }))
        }
        TokenKind::Placeholder => {
            s.advance();
            let name = s.interner.intern("_");
            Ok(s.expr(range, ExprKind::Identifier { name }))
        }

        TokenKind::This => {
            s.advance();
            Ok(s.expr(range, ExprKind::This))
        }
        TokenKind::Super => {
            s.advance();
            Ok(s.expr(range, ExprKind::Super))
        }

        TokenKind::LBracket => parse_array_expr(s),
        TokenKind::LBrace => parse_object_expr(s),

        TokenKind::LParen => {
            let start_range = s.range();
            s.advance();
            if s.eat(TokenKind::RParen) {
                let full_range = s.span_from(start_range);
                return Ok(s.expr(full_range, ExprKind::Tuple { elements: vec![] }));
            }
            let expr = parse_seq_expr(s)?;
            s.expect(TokenKind::RParen)?;
            let full_range = s.span_from(start_range);
            Ok(s.expr(full_range, ExprKind::Paren { expression: expr }))
        }

        TokenKind::New => parse_new_expr(s, range),
        TokenKind::Function => parse_function_expr(s),

        TokenKind::Async => {
            let save = s.save();
            if let Ok(Some(arrow)) = try_parse_arrow(s) {
                return Ok(arrow);
            }
            s.restore(save);
            let start_range = s.range();
            s.advance();
            s.expect(TokenKind::Function)?;
            parse_function_expr_inner_with_start(s, true, start_range)
        }

        TokenKind::Class => parse_class_expr(s),
        TokenKind::Match => parse_match_expr(s),

        TokenKind::Hash => {
            let start_range = s.range();
            if s.peek_kind(1) == TokenKind::LBracket {
                s.advance();
                s.advance();
                let mut elements = vec![];
                while !s.check(TokenKind::RBracket) && !s.is_eof() {
                    elements.push(super::parse_assign_expr(s)?);
                    s.eat(TokenKind::Comma);
                }
                s.expect(TokenKind::RBracket)?;
                let full_range = s.span_from(start_range);
                Ok(s.expr(full_range, ExprKind::Tuple { elements }))
            } else if s.peek_kind(1) == TokenKind::LBrace {
                s.advance();
                let obj = parse_object_expr(s)?;
                let full_range = s.span_from(start_range);
                let ExprKind::Object { properties } = s.arena.expr(obj).kind.clone() else {
                    unreachable!()
                };
                Ok(s.expr(full_range, ExprKind::Record { properties }))
            } else {
                Err(format!("Unexpected `#` at {}:{}", s.line(), s.column()))
            }
        }

        TokenKind::EOF
        | TokenKind::Dynamic
        | TokenKind::TemplateMiddle
        | TokenKind::TemplateTail
        | TokenKind::RParen
        | TokenKind::RBrace
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
        | TokenKind::Delete
        | TokenKind::Typeof
        | TokenKind::Instanceof
        | TokenKind::In
        | TokenKind::Of
        | TokenKind::Void
        | TokenKind::Is
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
        | TokenKind::At
        | TokenKind::Backslash
        | TokenKind::Dollar
        | TokenKind::Backtick
        | TokenKind::Newline
        | TokenKind::Whitespace
        | TokenKind::DocComment
        | TokenKind::Spawn
        | TokenKind::Parallel
        | TokenKind::Start => {
            let kind = s.kind();
            if kind.can_be_identifier() {
                let name = s.consume_lexeme();
                Ok(s.expr(range, ExprKind::Identifier { name }))
            } else {
                Err(format!(
                    "Unexpected token {:?} ({:?}) in expression at {}:{}",
                    s.kind(),
                    s.lexeme(),
                    s.line(),
                    s.column()
                ))
            }
        }
    }
}
