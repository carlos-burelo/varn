use super::kind::TokenKind;
use crate::source::SourceRange;

impl TokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            TokenKind::EOF => "",
            TokenKind::Identifier => "identifier",
            TokenKind::IntegerLiteral => "integer",
            TokenKind::FloatLiteral => "float",
            TokenKind::BigIntLiteral => "bigint",
            TokenKind::DecimalLiteral => "decimal",
            TokenKind::Str => "str",
            TokenKind::Char => "char",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LBracket => "[",
            TokenKind::RBracket => "]",
            TokenKind::LAngle => "<",
            TokenKind::RAngle => ">",
            TokenKind::Semicolon => ";",
            TokenKind::Comma => ",",
            TokenKind::Dot => ".",
            TokenKind::DotDot => "..",
            TokenKind::DotDotDot => "...",
            TokenKind::DotDotEq => "..=",
            TokenKind::Colon => ":",
            TokenKind::ColonColon => "::",
            TokenKind::Question => "?",
            TokenKind::QuestionDot => "?.",
            TokenKind::QuestionQuestion => "??",
            TokenKind::Plus => "+",
            TokenKind::PlusPlus => "++",
            TokenKind::PlusEq => "+=",
            TokenKind::Minus => "-",
            TokenKind::MinusMinus => "--",
            TokenKind::MinusEq => "-=",
            TokenKind::Star => "*",
            TokenKind::StarStar => "**",
            TokenKind::StarEq => "*=",
            TokenKind::Slash => "/",
            TokenKind::SlashEq => "/=",
            TokenKind::Percent => "%",
            TokenKind::PercentEq => "%=",
            TokenKind::Amp => "&",
            TokenKind::AmpAmp => "&&",
            TokenKind::AmpEq => "&=",
            TokenKind::Pipe => "|",
            TokenKind::PipePipe => "||",
            TokenKind::PipeEq => "|=",
            TokenKind::PipeGt => "|>",
            TokenKind::Caret => "^",
            TokenKind::CaretEq => "^=",
            TokenKind::Tilde => "~",
            TokenKind::Eq => "=",
            TokenKind::EqEq => "==",
            TokenKind::EqEqEq => "===",
            TokenKind::Bang => "!",
            TokenKind::BangEq => "!=",
            TokenKind::BangEqEq => "!==",
            TokenKind::Lt => "<",
            TokenKind::LtEq => "<=",
            TokenKind::Gt => ">",
            TokenKind::GtEq => ">=",
            TokenKind::Arrow => "->",
            TokenKind::FatArrow => "=>",
            TokenKind::Let
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
            | TokenKind::Placeholder => match self.keyword_text() {
                Some(text) => text,
                None => crate::UNKNOWN,
            },
            TokenKind::At => "@",
            TokenKind::RawStr => "string",
            TokenKind::Dynamic
            | TokenKind::BinaryLiteral
            | TokenKind::OctalLiteral
            | TokenKind::HexLiteral
            | TokenKind::Template
            | TokenKind::TemplateHead
            | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail
            | TokenKind::RegularExpression
            | TokenKind::QuestionLBracket
            | TokenKind::QuestionQuestionEq
            | TokenKind::StarStarEq
            | TokenKind::AmpAmpEq
            | TokenKind::PipePipeEq
            | TokenKind::LtLt
            | TokenKind::LtLtEq
            | TokenKind::GtGt
            | TokenKind::GtGtEq
            | TokenKind::GtGtGt
            | TokenKind::GtGtGtEq
            | TokenKind::Hash
            | TokenKind::Backslash
            | TokenKind::Dollar
            | TokenKind::Backtick
            | TokenKind::Newline
            | TokenKind::Whitespace
            | TokenKind::DocComment
            | TokenKind::Spawn
            | TokenKind::Parallel
            | TokenKind::Start => crate::UNKNOWN,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ParsedNumber {
    Int(i64),
    Float(f64),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub range: SourceRange,

    pub parsed_num: Option<ParsedNumber>,

    pub lex_start: u32,
    pub lex_len: u32,
}

impl Token {
    pub fn new(kind: TokenKind, range: SourceRange, lex_start: u32, lex_len: u32) -> Self {
        Token {
            kind,
            range,
            parsed_num: None,
            lex_start,
            lex_len,
        }
    }

    pub fn is(&self, kind: TokenKind) -> bool {
        self.kind == kind
    }

    #[inline]
    pub fn get_lexeme<'a>(&self, buf: &'a [u8]) -> &'a str {
        let start = self.lex_start as usize;
        let end = (self.lex_start + self.lex_len) as usize;
        std::str::from_utf8(&buf[start..end]).unwrap_or("")
    }
}
