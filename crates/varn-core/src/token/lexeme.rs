use super::kind::TokenKind;
use crate::source::SourceRange;

impl TokenKind {
    pub fn as_str(self) -> &'static str {
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
            TokenKind::Let => "let",
            TokenKind::Const => "const",
            TokenKind::Var => "var",
            TokenKind::Function => "function",
            TokenKind::Class => "class",
            TokenKind::Struct => "struct",
            TokenKind::Interface => "interface",
            TokenKind::Type => "type",
            TokenKind::Enum => "enum",
            TokenKind::Namespace => "namespace",
            TokenKind::Module => "module",
            TokenKind::Extension => "extension",
            TokenKind::If => "if",
            TokenKind::Else => "else",
            TokenKind::Switch => "switch",
            TokenKind::Case => "case",
            TokenKind::Default => "default",
            TokenKind::While => "while",
            TokenKind::For => "for",
            TokenKind::Do => "do",
            TokenKind::Break => "break",
            TokenKind::Continue => "continue",
            TokenKind::Return => "return",
            TokenKind::Throw => "throw",
            TokenKind::Try => "try",
            TokenKind::Catch => "catch",
            TokenKind::Finally => "finally",
            TokenKind::Using => "using",
            TokenKind::With => "with",
            TokenKind::Import => "import",
            TokenKind::Export => "export",
            TokenKind::From => "from",
            TokenKind::As => "as",
            TokenKind::Async => "async",
            TokenKind::Await => "await",
            TokenKind::Yield => "yield",
            TokenKind::New => "new",
            TokenKind::This => "this",
            TokenKind::Super => "super",
            TokenKind::Delete => "delete",
            TokenKind::Typeof => "typeof",
            TokenKind::Instanceof => "instanceof",
            TokenKind::In => "in",
            TokenKind::Of => "of",
            TokenKind::Void => "void",
            TokenKind::Is => "is",
            TokenKind::True => "true",
            TokenKind::False => "false",
            TokenKind::Null => "null",
            TokenKind::Public => "public",
            TokenKind::Private => "private",
            TokenKind::Protected => "protected",
            TokenKind::Static => "static",
            TokenKind::Abstract => "abstract",
            TokenKind::Override => "override",
            TokenKind::Readonly => "readonly",
            TokenKind::Declare => "declare",
            TokenKind::Native => "native",
            TokenKind::Extends => "extends",
            TokenKind::Implements => "implements",
            TokenKind::Get => "get",
            TokenKind::Set => "set",
            TokenKind::Constructor => "constructor",
            TokenKind::Destructor => "destructor",
            TokenKind::Match => "match",
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
            | TokenKind::On
            | TokenKind::Hash
            | TokenKind::Backslash
            | TokenKind::Dollar
            | TokenKind::Backtick
            | TokenKind::Newline
            | TokenKind::Whitespace
            | TokenKind::DocComment
            | TokenKind::Placeholder
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
