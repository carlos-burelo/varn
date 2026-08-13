use tower_lsp::lsp_types::{CompletionItemKind, SymbolKind as LspSymbolKind};
use varn_checker::SymbolKind;

use crate::document::MemberKind;

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

pub fn member_to_symbol_kind(kind: MemberKind) -> SymbolKind {
    match kind {
        MemberKind::Constructor => SymbolKind::Method,
        MemberKind::Method => SymbolKind::Method,
        MemberKind::Function => SymbolKind::Function,
        MemberKind::Property => SymbolKind::Property,
        MemberKind::Variable => SymbolKind::Var,
        MemberKind::EnumMember => SymbolKind::EnumMember,
        MemberKind::Getter => SymbolKind::Property,
        MemberKind::Setter => SymbolKind::Property,
        MemberKind::Class => SymbolKind::Class,
        MemberKind::Interface => SymbolKind::Interface,
        MemberKind::Namespace => SymbolKind::Namespace,
        MemberKind::Enum => SymbolKind::Enum,
        MemberKind::Struct => SymbolKind::Struct,
    }
}

pub fn member_to_completion_kind(kind: MemberKind) -> CompletionItemKind {
    match kind {
        MemberKind::Property | MemberKind::Variable | MemberKind::Getter | MemberKind::Setter => {
            CompletionItemKind::PROPERTY
        }
        MemberKind::Method => CompletionItemKind::METHOD,
        MemberKind::Function => CompletionItemKind::FUNCTION,
        MemberKind::Constructor => CompletionItemKind::CONSTRUCTOR,
        MemberKind::Class => CompletionItemKind::CLASS,
        MemberKind::Interface => CompletionItemKind::INTERFACE,
        MemberKind::Namespace => CompletionItemKind::MODULE,
        MemberKind::Enum => CompletionItemKind::ENUM,
        MemberKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
        MemberKind::Struct => CompletionItemKind::STRUCT,
    }
}

pub fn member_kind_label(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::Class => "class",
        MemberKind::Interface => "interface",
        MemberKind::Namespace => "namespace",
        MemberKind::Enum | MemberKind::EnumMember => "enum",
        MemberKind::Struct => "struct",
        MemberKind::Property => "prop",
        MemberKind::Variable => "var",
        MemberKind::Method => "method",
        MemberKind::Function => "function",
        MemberKind::Getter => "get",
        MemberKind::Setter => "set",
        MemberKind::Constructor => "constructor",
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
