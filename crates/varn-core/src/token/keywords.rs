use super::kind::TokenKind;

macro_rules! keywords {
    ($($kind:ident => $text:literal,)*) => {
        impl TokenKind {
            pub const KEYWORDS: &'static [(TokenKind, &'static str)] =
                &[$((TokenKind::$kind, $text),)*];

            pub fn keyword(text: &str) -> Option<TokenKind> {
                match text {
                    $($text => Some(TokenKind::$kind),)*
                    _ => None,
                }
            }
        }
    };
}

keywords! {
    Let => "let",
    Const => "const",
    Var => "var",
    Function => "function",
    Class => "class",
    Struct => "struct",
    Interface => "interface",
    Type => "type",
    Enum => "enum",
    Namespace => "namespace",
    Module => "module",
    Extension => "extension",
    On => "on",
    If => "if",
    Else => "else",
    Switch => "switch",
    Case => "case",
    Default => "default",
    While => "while",
    For => "for",
    Do => "do",
    Break => "break",
    Continue => "continue",
    Return => "return",
    Throw => "throw",
    Try => "try",
    Catch => "catch",
    Finally => "finally",
    Using => "using",
    With => "with",
    Import => "import",
    Export => "export",
    From => "from",
    As => "as",
    Async => "async",
    Await => "await",
    Yield => "yield",
    New => "new",
    This => "this",
    Super => "super",
    Delete => "delete",
    Typeof => "typeof",
    Instanceof => "instanceof",
    In => "in",
    Of => "of",
    Void => "void",
    Is => "is",
    True => "true",
    False => "false",
    Null => "null",
    Public => "public",
    Private => "private",
    Protected => "protected",
    Static => "static",
    Abstract => "abstract",
    Override => "override",
    Readonly => "readonly",
    Declare => "declare",
    Native => "native",
    Extends => "extends",
    Implements => "implements",
    Get => "get",
    Set => "set",
    Constructor => "constructor",
    Destructor => "destructor",
    Match => "match",
    Placeholder => "_",
}

impl TokenKind {
    pub const fn keyword_text(self) -> Option<&'static str> {
        let mut i = 0;
        while i < Self::KEYWORDS.len() {
            if Self::KEYWORDS[i].0 as u32 == self as u32 {
                return Some(Self::KEYWORDS[i].1);
            }
            i += 1;
        }
        None
    }
}
