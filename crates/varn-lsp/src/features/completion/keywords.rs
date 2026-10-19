use varn_core::TokenKind;

pub struct KwEntry {
    pub label: &'static str,
    pub detail: Option<&'static str>,
    pub doc: Option<&'static str>,
    pub snippet: Option<&'static str>,
}

pub const fn kw(kind: TokenKind) -> KwEntry {
    KwEntry {
        label: kind.as_str(),
        detail: None,
        doc: None,
        snippet: None,
    }
}

pub const fn kw_snip(kind: TokenKind, detail: &'static str, snippet: &'static str) -> KwEntry {
    KwEntry {
        label: kind.as_str(),
        detail: Some(detail),
        doc: None,
        snippet: Some(snippet),
    }
}

pub static KEYWORDS: &[KwEntry] = &[
    kw(TokenKind::Let),
    kw(TokenKind::Const),
    kw(TokenKind::Var),
    kw_snip(
        TokenKind::Function,
        "fn declaration",
        "function ${1:name}(${2:params}) {\n\t$0\n}",
    ),
    kw_snip(
        TokenKind::Class,
        "class declaration",
        "class ${1:Name} {\n\t$0\n}",
    ),
    kw_snip(
        TokenKind::Struct,
        "struct declaration",
        "struct ${1:Name} {\n\t$0\n}",
    ),
    kw_snip(
        TokenKind::Interface,
        "interface",
        "interface ${1:Name} {\n\t$0\n}",
    ),
    kw(TokenKind::Type),
    kw(TokenKind::Enum),
    kw(TokenKind::Namespace),
    kw(TokenKind::Module),
    kw(TokenKind::Extension),
    kw(TokenKind::On),
    kw_snip(
        TokenKind::If,
        "conditional",
        "if (${1:condition}) {\n\t$0\n}",
    ),
    kw(TokenKind::Else),
    kw_snip(
        TokenKind::Switch,
        "switch",
        "switch (${1:expr}) {\n\tcase ${2:value}:\n\t\t$0\n}",
    ),
    kw(TokenKind::Case),
    kw(TokenKind::Default),
    kw_snip(
        TokenKind::While,
        "while loop",
        "while (${1:condition}) {\n\t$0\n}",
    ),
    kw_snip(
        TokenKind::For,
        "for loop",
        "for (${1:init}; ${2:cond}; ${3:update}) {\n\t$0\n}",
    ),
    kw(TokenKind::Do),
    kw(TokenKind::Break),
    kw(TokenKind::Continue),
    kw(TokenKind::Return),
    kw(TokenKind::Throw),
    kw_snip(
        TokenKind::Try,
        "try/catch",
        "try {\n\t$1\n} catch (${2:e}) {\n\t$0\n}",
    ),
    kw(TokenKind::Catch),
    kw(TokenKind::Finally),
    kw(TokenKind::Using),
    kw(TokenKind::Import),
    kw(TokenKind::Export),
    kw(TokenKind::From),
    kw(TokenKind::As),
    kw(TokenKind::Async),
    kw(TokenKind::Await),
    kw(TokenKind::Yield),
    kw(TokenKind::New),
    kw(TokenKind::This),
    kw(TokenKind::Super),
    kw(TokenKind::Delete),
    kw(TokenKind::Typeof),
    kw(TokenKind::Instanceof),
    kw(TokenKind::In),
    kw(TokenKind::Of),
    kw(TokenKind::Void),
    kw(TokenKind::Is),
    kw(TokenKind::True),
    kw(TokenKind::False),
    kw(TokenKind::Null),
    kw(TokenKind::Public),
    kw(TokenKind::Private),
    kw(TokenKind::Protected),
    kw(TokenKind::Static),
    kw(TokenKind::Abstract),
    kw(TokenKind::Override),
    kw(TokenKind::Readonly),
    kw(TokenKind::Declare),
    kw(TokenKind::Native),
    kw(TokenKind::Extends),
    kw(TokenKind::Implements),
    kw(TokenKind::Get),
    kw(TokenKind::Set),
    kw(TokenKind::Constructor),
    kw(TokenKind::Destructor),
    kw_snip(
        TokenKind::Match,
        "pattern match",
        "match (${1:expr}) {\n\t${2:pattern} => $0\n}",
    ),
];
