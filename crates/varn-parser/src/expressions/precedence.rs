use varn_core::TokenKind;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Prec {
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

pub(crate) fn binary_prec(kind: TokenKind) -> Option<(Prec, bool)> {
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
