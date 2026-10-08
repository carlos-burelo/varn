mod calls;
mod literal_text;
mod ops;
mod primary;

pub use calls::{parse_call_args, parse_call_args_pub, parse_new_callee_expr, parse_unary_expr};

use crate::stream::TokenStream;
use crate::types::parse_type;
use ops::{could_be_arrow, parse_yield_expr, try_parse_arrow};
use varn_core::ast::operators::{AssignOp, BinaryOp, LogicalOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TokenKind;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Prec {
    None,
    NullCoalesce,
    LogicalOr,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equality,
    Relational,
    Pipe,
    Range,
    Shift,
    Additive,
    Multiplicative,
    Exponent,
}

fn binary_prec(kind: TokenKind) -> Option<(Prec, bool)> {
    let (p, r) = match kind {
        TokenKind::PipePipe => (Prec::LogicalOr, false),
        TokenKind::AmpAmp => (Prec::LogicalAnd, false),
        TokenKind::Pipe => (Prec::BitwiseOr, false),
        TokenKind::Caret => (Prec::BitwiseXor, false),
        TokenKind::Amp => (Prec::BitwiseAnd, false),
        TokenKind::EqEq | TokenKind::EqEqEq | TokenKind::BangEq | TokenKind::BangEqEq => {
            (Prec::Equality, false)
        }
        TokenKind::LAngle
        | TokenKind::RAngle
        | TokenKind::LtEq
        | TokenKind::GtEq
        | TokenKind::Instanceof
        | TokenKind::In => (Prec::Relational, false),
        TokenKind::PipeGt => (Prec::Pipe, false),
        TokenKind::DotDot | TokenKind::DotDotEq => (Prec::Range, false),
        TokenKind::LtLt | TokenKind::GtGt | TokenKind::GtGtGt => (Prec::Shift, false),
        TokenKind::Plus | TokenKind::Minus => (Prec::Additive, false),
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => (Prec::Multiplicative, false),
        TokenKind::StarStar => (Prec::Exponent, true),
        TokenKind::QuestionQuestion => (Prec::NullCoalesce, false),
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
        | TokenKind::Semicolon
        | TokenKind::Comma
        | TokenKind::Dot
        | TokenKind::DotDotDot
        | TokenKind::Colon
        | TokenKind::ColonColon
        | TokenKind::Question
        | TokenKind::QuestionDot
        | TokenKind::QuestionLBracket
        | TokenKind::QuestionQuestionEq
        | TokenKind::PlusPlus
        | TokenKind::PlusEq
        | TokenKind::MinusMinus
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::StarStarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::AmpEq
        | TokenKind::AmpAmpEq
        | TokenKind::PipeEq
        | TokenKind::PipePipeEq
        | TokenKind::CaretEq
        | TokenKind::Tilde
        | TokenKind::LtLtEq
        | TokenKind::GtGtEq
        | TokenKind::GtGtGtEq
        | TokenKind::Eq
        | TokenKind::Bang
        | TokenKind::Lt
        | TokenKind::Gt
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
        | TokenKind::RawStr => return None,
    };
    Some((p, r))
}

fn token_to_binary_op(kind: TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::Plus => Some(BinaryOp::Add),
        TokenKind::Minus => Some(BinaryOp::Sub),
        TokenKind::Star => Some(BinaryOp::Mul),
        TokenKind::Slash => Some(BinaryOp::Div),
        TokenKind::Percent => Some(BinaryOp::Mod),
        TokenKind::StarStar => Some(BinaryOp::Pow),
        TokenKind::EqEq | TokenKind::EqEqEq => Some(BinaryOp::Eq),
        TokenKind::BangEq | TokenKind::BangEqEq => Some(BinaryOp::NotEq),
        TokenKind::LAngle => Some(BinaryOp::Lt),
        TokenKind::RAngle => Some(BinaryOp::Gt),
        TokenKind::LtEq => Some(BinaryOp::LtEq),
        TokenKind::GtEq => Some(BinaryOp::GtEq),
        TokenKind::Amp => Some(BinaryOp::BitAnd),
        TokenKind::Pipe => Some(BinaryOp::BitOr),
        TokenKind::Caret => Some(BinaryOp::BitXor),
        TokenKind::LtLt => Some(BinaryOp::Shl),
        TokenKind::GtGt => Some(BinaryOp::Shr),
        TokenKind::GtGtGt => Some(BinaryOp::UShr),
        TokenKind::Instanceof => Some(BinaryOp::Instanceof),
        TokenKind::In => Some(BinaryOp::In),
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
        | TokenKind::PlusPlus
        | TokenKind::PlusEq
        | TokenKind::MinusMinus
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::StarStarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::AmpAmp
        | TokenKind::AmpEq
        | TokenKind::AmpAmpEq
        | TokenKind::PipePipe
        | TokenKind::PipeEq
        | TokenKind::PipePipeEq
        | TokenKind::PipeGt
        | TokenKind::CaretEq
        | TokenKind::Tilde
        | TokenKind::LtLtEq
        | TokenKind::GtGtEq
        | TokenKind::GtGtGtEq
        | TokenKind::Eq
        | TokenKind::Bang
        | TokenKind::Lt
        | TokenKind::Gt
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
        | TokenKind::RawStr => None,
    }
}

fn token_to_logical_op(kind: TokenKind) -> Option<LogicalOp> {
    match kind {
        TokenKind::AmpAmp => Some(LogicalOp::And),
        TokenKind::PipePipe => Some(LogicalOp::Or),
        TokenKind::QuestionQuestion => Some(LogicalOp::Nullish),
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
        | TokenKind::AmpEq
        | TokenKind::AmpAmpEq
        | TokenKind::Pipe
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
        | TokenKind::RawStr => None,
    }
}

fn token_to_assign_op(kind: TokenKind) -> Option<AssignOp> {
    match kind {
        TokenKind::Eq => Some(AssignOp::Assign),
        TokenKind::PlusEq => Some(AssignOp::AddAssign),
        TokenKind::MinusEq => Some(AssignOp::SubAssign),
        TokenKind::StarEq => Some(AssignOp::MulAssign),
        TokenKind::SlashEq => Some(AssignOp::DivAssign),
        TokenKind::PercentEq => Some(AssignOp::ModAssign),
        TokenKind::StarStarEq => Some(AssignOp::PowAssign),
        TokenKind::AmpEq => Some(AssignOp::BitAndAssign),
        TokenKind::PipeEq => Some(AssignOp::BitOrAssign),
        TokenKind::CaretEq => Some(AssignOp::BitXorAssign),
        TokenKind::LtLtEq => Some(AssignOp::ShlAssign),
        TokenKind::GtGtEq => Some(AssignOp::ShrAssign),
        TokenKind::GtGtGtEq => Some(AssignOp::UShrAssign),
        TokenKind::AmpAmpEq => Some(AssignOp::AndAssign),
        TokenKind::PipePipeEq => Some(AssignOp::OrAssign),
        TokenKind::QuestionQuestionEq => Some(AssignOp::NullishAssign),
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
        | TokenKind::Plus
        | TokenKind::PlusPlus
        | TokenKind::Minus
        | TokenKind::MinusMinus
        | TokenKind::Star
        | TokenKind::StarStar
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::Amp
        | TokenKind::AmpAmp
        | TokenKind::Pipe
        | TokenKind::PipePipe
        | TokenKind::PipeGt
        | TokenKind::Caret
        | TokenKind::Tilde
        | TokenKind::LtLt
        | TokenKind::GtGt
        | TokenKind::GtGtGt
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
        | TokenKind::RawStr => None,
    }
}

pub fn parse_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    parse_assign_expr(s)
}

pub fn parse_seq_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let start = s.range();
    let first = parse_assign_expr(s)?;
    if !s.check(TokenKind::Comma) {
        return Ok(first);
    }
    let mut exprs = vec![first];
    while s.eat(TokenKind::Comma) {
        exprs.push(parse_assign_expr(s)?);
    }
    let range = if let Some(last) = exprs.last() {
        start.to(s.expr_range(*last))
    } else {
        start
    };
    Ok(s.expr(range, ExprKind::Sequence { expressions: exprs }))
}

pub(super) fn parse_assign_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    if s.check(TokenKind::Yield) {
        return parse_yield_expr(s);
    }

    if could_be_arrow(s) {
        if let Some(arrow) = try_parse_arrow(s)? {
            return Ok(arrow);
        }
    }

    let left = parse_conditional_expr(s)?;

    if let Some(op) = token_to_assign_op(s.kind()) {
        s.advance();
        let right = parse_assign_expr(s)?;
        let range = s.expr_range(left).to(s.expr_range(right));
        return Ok(s.expr(
            range,
            ExprKind::Assign {
                op,
                target: left,
                value: right,
            },
        ));
    }

    Ok(left)
}

fn parse_conditional_expr(s: &mut TokenStream) -> Result<ExprId, String> {
    let expr = parse_binary_expr(s, Prec::None)?;

    if s.eat(TokenKind::Question) {
        let consequent = parse_assign_expr(s)?;
        s.expect(TokenKind::Colon)?;
        let alternate = parse_assign_expr(s)?;
        let range = s.expr_range(expr).to(s.expr_range(alternate));
        return Ok(s.expr(
            range,
            ExprKind::Conditional {
                test: expr,
                consequent,
                alternate,
            },
        ));
    }

    Ok(expr)
}

pub(super) fn parse_binary_expr(s: &mut TokenStream, min_prec: Prec) -> Result<ExprId, String> {
    let mut left = parse_unary_expr(s)?;

    loop {
        let kind = s.kind();

        if let Some((prec, right_assoc)) = binary_prec(kind) {
            if prec <= min_prec {
                break;
            }
            let op_kind = kind;
            s.advance();
            let next_min = if right_assoc {
                Prec::Multiplicative
            } else {
                prec
            };
            let right = parse_binary_expr(s, next_min)?;

            let range = s.expr_range(left).to(s.expr_range(right));
            if let Some(logical) = token_to_logical_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Logical {
                        op: logical,
                        left,
                        right,
                    },
                );
            } else if op_kind == TokenKind::DotDot || op_kind == TokenKind::DotDotEq {
                left = s.expr(
                    range,
                    ExprKind::Range {
                        start: left,
                        end: right,
                        inclusive: op_kind == TokenKind::DotDotEq,
                    },
                );
            } else if op_kind == TokenKind::PipeGt {
                left = s.expr(range, ExprKind::Pipeline { left, right });
            } else if let Some(bin) = token_to_binary_op(op_kind) {
                left = s.expr(
                    range,
                    ExprKind::Binary {
                        op: bin,
                        left,
                        right,
                    },
                );
            }
            continue;
        }

        if kind == TokenKind::As
            || kind == TokenKind::Is
            || (kind == TokenKind::Identifier && s.lexeme() == "satisfies")
        {
            s.advance();
            let ty = parse_type(s)?;
            let ty_range = *ty.clone().range();
            let range = s.expr_range(left).to(ty_range);
            left = if kind == TokenKind::As {
                s.expr(
                    range,
                    ExprKind::As {
                        expression: left,
                        type_ann: ty,
                    },
                )
            } else if kind == TokenKind::Is {
                s.expr(
                    range,
                    ExprKind::Is {
                        expression: left,
                        type_ann: ty,
                    },
                )
            } else {
                s.expr(
                    range,
                    ExprKind::Satisfies {
                        expression: left,
                        type_ann: ty,
                    },
                )
            };
            continue;
        }

        break;
    }

    Ok(left)
}
