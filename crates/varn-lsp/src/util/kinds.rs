use tower_lsp::lsp_types::{CompletionItemKind, SymbolKind as LspSymbolKind};
use varn_checker::SymbolKind;

pub fn to_lsp_symbol_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Let | SymbolKind::Var => LspSymbolKind::VARIABLE,
        SymbolKind::Const => LspSymbolKind::CONSTANT,
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Class => LspSymbolKind::CLASS,
        SymbolKind::Interface => LspSymbolKind::INTERFACE,
        SymbolKind::TypeAlias => LspSymbolKind::TYPE_PARAMETER,
        SymbolKind::Enum => LspSymbolKind::ENUM,
        SymbolKind::Parameter => LspSymbolKind::VARIABLE,
        SymbolKind::Property => LspSymbolKind::PROPERTY,
        SymbolKind::Method => LspSymbolKind::METHOD,
        SymbolKind::TypeParameter => LspSymbolKind::TYPE_PARAMETER,
        SymbolKind::Namespace => LspSymbolKind::NAMESPACE,
        SymbolKind::Struct => LspSymbolKind::STRUCT,
        SymbolKind::Extension => LspSymbolKind::CLASS,
        SymbolKind::EnumMember => LspSymbolKind::ENUM_MEMBER,
    }
}

pub fn to_completion_kind(kind: SymbolKind) -> CompletionItemKind {
    match kind {
        SymbolKind::Let | SymbolKind::Var => CompletionItemKind::VARIABLE,
        SymbolKind::Const => CompletionItemKind::CONSTANT,
        SymbolKind::Function => CompletionItemKind::FUNCTION,
        SymbolKind::Class => CompletionItemKind::CLASS,
        SymbolKind::Interface => CompletionItemKind::INTERFACE,
        SymbolKind::TypeAlias => CompletionItemKind::TYPE_PARAMETER,
        SymbolKind::Enum => CompletionItemKind::ENUM,
        SymbolKind::Parameter => CompletionItemKind::VARIABLE,
        SymbolKind::Property => CompletionItemKind::PROPERTY,
        SymbolKind::Method => CompletionItemKind::METHOD,
        SymbolKind::TypeParameter => CompletionItemKind::TYPE_PARAMETER,
        SymbolKind::Namespace => CompletionItemKind::MODULE,
        SymbolKind::Struct => CompletionItemKind::STRUCT,
        SymbolKind::Extension => CompletionItemKind::CLASS,
        SymbolKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
    }
}

pub fn is_container_symbol_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Namespace
            | SymbolKind::Enum
            | SymbolKind::Struct
    )
}
