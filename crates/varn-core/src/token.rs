use crate::source::SourceRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum TokenKind {
    EOF = 0,
    Dynamic = 1,
    Identifier = 2,
    IntegerLiteral = 3,
    FloatLiteral = 4,
    BinaryLiteral = 5,
    OctalLiteral = 6,
    HexLiteral = 7,
    BigIntLiteral = 8,
    Str = 9,
    Char = 10,
    Template = 11,
    TemplateHead = 12,
    TemplateMiddle = 13,
    TemplateTail = 14,
    RegularExpression = 15,
    LParen = 16,
    RParen = 17,
    LBrace = 18,
    RBrace = 19,
    LBracket = 20,
    RBracket = 21,
    LAngle = 22,
    RAngle = 23,
    Semicolon = 24,
    Comma = 25,
    Dot = 26,
    DotDot = 27,
    DotDotDot = 28,
    DotDotEq = 29,
    Colon = 30,
    ColonColon = 31,
    Question = 32,
    QuestionDot = 33,
    QuestionLBracket = 34,
    QuestionQuestion = 35,
    QuestionQuestionEq = 36,
    Plus = 37,
    PlusPlus = 38,
    PlusEq = 39,
    Minus = 40,
    MinusMinus = 41,
    MinusEq = 42,
    Star = 43,
    StarStar = 44,
    StarEq = 45,
    StarStarEq = 46,
    Slash = 47,
    SlashEq = 48,
    Percent = 49,
    PercentEq = 50,
    Amp = 51,
    AmpAmp = 52,
    AmpEq = 53,
    AmpAmpEq = 54,
    Pipe = 55,
    PipePipe = 56,
    PipeEq = 57,
    PipePipeEq = 58,
    PipeGt = 59,
    Caret = 60,
    CaretEq = 61,
    Tilde = 62,
    LtLt = 63,
    LtLtEq = 64,
    GtGt = 65,
    GtGtEq = 66,
    GtGtGt = 67,
    GtGtGtEq = 68,
    Eq = 69,
    EqEq = 70,
    EqEqEq = 71,
    Bang = 72,
    BangEq = 73,
    BangEqEq = 74,
    Lt = 75,
    LtEq = 76,
    Gt = 77,
    GtEq = 78,
    Arrow = 79,
    FatArrow = 80,
    Let = 81,
    Const = 82,
    Var = 83,
    Function = 84,
    Class = 85,
    Struct = 86,
    Interface = 87,
    Type = 88,
    Enum = 89,
    Namespace = 90,
    Module = 91,
    Extension = 92,
    On = 93,
    If = 94,
    Else = 95,
    Switch = 96,
    Case = 97,
    Default = 98,
    While = 99,
    For = 100,
    Do = 101,
    Break = 102,
    Continue = 103,
    Return = 104,
    Throw = 105,
    Try = 106,
    Catch = 107,
    Finally = 108,
    Using = 109,
    With = 110,
    Import = 111,
    Export = 112,
    From = 113,
    As = 114,
    Async = 115,
    Await = 116,
    Yield = 117,
    New = 118,
    This = 119,
    Super = 120,
    Delete = 121,
    Typeof = 122,
    Instanceof = 123,
    In = 124,
    Of = 125,
    Void = 126,
    Is = 127,
    True = 128,
    False = 129,
    Null = 130,
    Public = 131,
    Private = 132,
    Protected = 133,
    Static = 134,
    Abstract = 135,
    Override = 136,
    Readonly = 137,
    Declare = 138,
    Native = 139,
    Extends = 140,
    Implements = 141,
    Get = 142,
    Set = 143,
    Constructor = 144,
    Destructor = 145,
    Match = 146,
    At = 147,
    Hash = 148,
    Backslash = 149,
    Dollar = 150,
    Backtick = 151,
    Newline = 152,
    Whitespace = 153,

    DocComment = 154,

    Placeholder = 155,

    DecimalLiteral = 156,

    Spawn = 157,

    Parallel = 158,

    Start = 159,
}

impl TokenKind {
    pub fn from_u32(v: u32) -> Self {
        if v <= 159 {
            unsafe { std::mem::transmute::<u32, TokenKind>(v) }
        } else {
            TokenKind::Dynamic
        }
    }

    pub fn is_keyword(self) -> bool {
        (self as u32) >= (TokenKind::Let as u32) && (self as u32) <= (TokenKind::Match as u32)
    }

    pub fn is_literal(self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            IntegerLiteral
                | FloatLiteral
                | BinaryLiteral
                | OctalLiteral
                | HexLiteral
                | BigIntLiteral
                | DecimalLiteral
                | Str
                | Char
                | Template
                | TemplateHead
                | TemplateMiddle
                | TemplateTail
                | RegularExpression
                | True
                | False
                | Null
        )
    }

    pub fn starts_statement(self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            If | While
                | For
                | Do
                | Return
                | Throw
                | Try
                | Break
                | Continue
                | Switch
                | Let
                | Const
                | Var
                | Function
                | Class
                | Struct
                | Interface
                | Type
                | Enum
                | Namespace
                | Import
                | Export
                | At
                | LBrace
                | Semicolon
                | Async
                | Declare
                | Abstract
                | Using
        )
    }

    pub fn can_be_identifier(self) -> bool {
        if self == TokenKind::Identifier {
            return true;
        }

        matches!(
            self,
            TokenKind::Get
                | TokenKind::Set
                | TokenKind::Async
                | TokenKind::Await
                | TokenKind::Yield
                | TokenKind::Type
                | TokenKind::Of
                | TokenKind::As
                | TokenKind::From
                | TokenKind::Static
                | TokenKind::Abstract
                | TokenKind::Override
                | TokenKind::Readonly
                | TokenKind::Declare
                | TokenKind::Native
                | TokenKind::Is
                | TokenKind::On
                | TokenKind::Namespace
                | TokenKind::Module
                | TokenKind::Extension
                | TokenKind::Constructor
                | TokenKind::Destructor
                | TokenKind::Placeholder
                | TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
        )
    }

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
            _ => "unknown",
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

    pub fn eof(range: SourceRange) -> Self {
        Token {
            kind: TokenKind::EOF,
            range,
            parsed_num: None,
            lex_start: 0,
            lex_len: 0,
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
