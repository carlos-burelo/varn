use vn_core::TokenKind;

pub const STATEMENT: u8 = 1;
pub const EXPRESSION: u8 = 2;
pub const MEMBER: u8 = 4;
pub const TYPE: u8 = 8;
pub const EXPORT: u8 = 16;
pub const INTERFACE_MEMBER: u8 = 32;

pub struct KwEntry {
    pub label: &'static str,
    pub detail: Option<&'static str>,
    pub doc: Option<&'static str>,
    pub snippet: Option<&'static str>,
    pub contexts: u8,
}

pub const fn kw(kind: TokenKind, contexts: u8) -> KwEntry {
    KwEntry {
        label: kind.as_str(),
        detail: None,
        doc: None,
        snippet: None,
        contexts,
    }
}

pub const fn kw_snip(
    kind: TokenKind,
    detail: &'static str,
    snippet: &'static str,
    contexts: u8,
) -> KwEntry {
    KwEntry {
        label: kind.as_str(),
        detail: Some(detail),
        doc: None,
        snippet: Some(snippet),
        contexts,
    }
}

pub static KEYWORDS: &[KwEntry] = &[
    kw(TokenKind::Let, STATEMENT | EXPORT),
    kw(TokenKind::Const, STATEMENT | EXPORT),
    kw(TokenKind::Using, STATEMENT),
    kw_snip(
        TokenKind::Function,
        "function declaration",
        "function ${1:name}(${2:params}) {\n\t$0\n}",
        STATEMENT | EXPORT,
    ),
    kw_snip(
        TokenKind::Class,
        "class declaration",
        "class ${1:Name} {\n\t$0\n}",
        STATEMENT | EXPORT,
    ),
    kw_snip(
        TokenKind::Struct,
        "struct declaration",
        "struct ${1:Name} {\n\t$0\n}",
        STATEMENT | EXPORT,
    ),
    kw_snip(
        TokenKind::Interface,
        "interface declaration",
        "interface ${1:Name} {\n\t$0\n}",
        STATEMENT | EXPORT,
    ),
    kw(TokenKind::Type, STATEMENT | EXPORT),
    kw(TokenKind::Enum, STATEMENT | EXPORT),
    kw(TokenKind::Namespace, STATEMENT | EXPORT),
    kw(TokenKind::Extension, STATEMENT | EXPORT),
    kw_snip(
        TokenKind::If,
        "conditional",
        "if (${1:condition}) {\n\t$0\n}",
        STATEMENT,
    ),
    kw(TokenKind::Else, STATEMENT),
    kw_snip(
        TokenKind::While,
        "while loop",
        "while (${1:condition}) {\n\t$0\n}",
        STATEMENT,
    ),
    kw_snip(
        TokenKind::For,
        "for loop",
        "for (const ${1:item} of ${2:items}) {\n\t$0\n}",
        STATEMENT,
    ),
    kw(TokenKind::Do, STATEMENT),
    kw(TokenKind::Break, STATEMENT),
    kw(TokenKind::Continue, STATEMENT),
    kw(TokenKind::Return, STATEMENT),
    kw(TokenKind::Throw, STATEMENT),
    kw_snip(
        TokenKind::Try,
        "try/catch",
        "try {\n\t$1\n} catch (${2:e}) {\n\t$0\n}",
        STATEMENT,
    ),
    kw(TokenKind::Catch, STATEMENT),
    kw(TokenKind::Finally, STATEMENT),
    kw(TokenKind::Import, STATEMENT),
    kw(TokenKind::Export, STATEMENT),
    kw(TokenKind::Async, STATEMENT | EXPRESSION | MEMBER | EXPORT),
    kw(TokenKind::Await, STATEMENT | EXPRESSION),
    kw(TokenKind::Yield, STATEMENT | EXPRESSION),
    kw(TokenKind::New, STATEMENT | EXPRESSION),
    kw(TokenKind::This, STATEMENT | EXPRESSION),
    kw(TokenKind::Super, STATEMENT | EXPRESSION),
    kw(TokenKind::True, EXPRESSION | TYPE),
    kw(TokenKind::False, EXPRESSION | TYPE),
    kw(TokenKind::Null, EXPRESSION | TYPE),
    kw(TokenKind::Void, TYPE),
    kw(TokenKind::Typeof, TYPE),
    kw(TokenKind::Public, MEMBER),
    kw(TokenKind::Private, MEMBER),
    kw(TokenKind::Protected, MEMBER),
    kw(TokenKind::Static, MEMBER),
    kw(TokenKind::Abstract, STATEMENT | MEMBER | EXPORT),
    kw(TokenKind::Override, MEMBER),
    kw(TokenKind::Readonly, MEMBER | TYPE | INTERFACE_MEMBER),
    kw(TokenKind::Mut, INTERFACE_MEMBER),
    kw(TokenKind::Declare, STATEMENT | EXPORT),
    kw(TokenKind::Native, STATEMENT | EXPORT),
    kw(TokenKind::Get, MEMBER),
    kw(TokenKind::Set, MEMBER),
    kw(TokenKind::Constructor, MEMBER),
    kw(TokenKind::Destructor, MEMBER),
    kw_snip(
        TokenKind::Match,
        "pattern match",
        "match (${1:expr}) {\n\t${2:pattern} => $0\n}",
        STATEMENT | EXPRESSION,
    ),
];
