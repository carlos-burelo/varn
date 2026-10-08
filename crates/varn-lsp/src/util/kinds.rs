use tower_lsp_f::lsp_types::{CompletionItemKind, SymbolKind as LspSymbolKind};
use varn_checker::SymbolKind;

pub fn to_lsp_symbol_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Let | SymbolKind::Var => LspSymbolKind::Variable,
        SymbolKind::Const => LspSymbolKind::Constant,
        SymbolKind::Function => LspSymbolKind::Function,
        SymbolKind::Class => LspSymbolKind::Class,
        SymbolKind::Interface => LspSymbolKind::Interface,
        SymbolKind::TypeAlias => LspSymbolKind::TypeParameter,
        SymbolKind::Enum => LspSymbolKind::Enum,
        SymbolKind::Parameter => LspSymbolKind::Variable,
        SymbolKind::Property => LspSymbolKind::Property,
        SymbolKind::Method => LspSymbolKind::Method,
        SymbolKind::TypeParameter => LspSymbolKind::TypeParameter,
        SymbolKind::Namespace => LspSymbolKind::Namespace,
        SymbolKind::Struct => LspSymbolKind::Struct,
        SymbolKind::Extension => LspSymbolKind::Class,
        SymbolKind::EnumMember => LspSymbolKind::EnumMember,
    }
}

pub fn to_completion_kind(kind: SymbolKind) -> CompletionItemKind {
    match kind {
        SymbolKind::Let | SymbolKind::Var => CompletionItemKind::Variable,
        SymbolKind::Const => CompletionItemKind::Constant,
        SymbolKind::Function => CompletionItemKind::Function,
        SymbolKind::Class => CompletionItemKind::Class,
        SymbolKind::Interface => CompletionItemKind::Interface,
        SymbolKind::TypeAlias => CompletionItemKind::TypeParameter,
        SymbolKind::Enum => CompletionItemKind::Enum,
        SymbolKind::Parameter => CompletionItemKind::Variable,
        SymbolKind::Property => CompletionItemKind::Property,
        SymbolKind::Method => CompletionItemKind::Method,
        SymbolKind::TypeParameter => CompletionItemKind::TypeParameter,
        SymbolKind::Namespace => CompletionItemKind::Module,
        SymbolKind::Struct => CompletionItemKind::Struct,
        SymbolKind::Extension => CompletionItemKind::Class,
        SymbolKind::EnumMember => CompletionItemKind::EnumMember,
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
