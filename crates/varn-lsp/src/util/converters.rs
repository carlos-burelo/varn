use tower_lsp::lsp_types::{DiagnosticSeverity, Position, Range};
use varn_core::DiagnosticKind;

#[inline]
pub fn ast_line_to_lsp(ast_line: u32) -> u32 {
    ast_line.saturating_sub(1)
}

#[inline]
pub fn lsp_line_to_ast(lsp_line: u32) -> u32 {
    lsp_line + 1
}

#[inline]
pub fn pos(line: u32, col: u32) -> Position {
    Position {
        line,
        character: col,
    }
}

#[inline]
pub fn range_on_line(line: u32, col_start: u32, col_end: u32) -> Range {
    Range {
        start: pos(line, col_start),
        end: pos(line, col_end),
    }
}

#[inline]
pub fn zero_range(line: u32, col: u32) -> Range {
    range_on_line(line, col, col)
}

use tower_lsp::lsp_types::SymbolKind as LspSymbolKind;
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

use tower_lsp::lsp_types::CompletionItemKind;

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

pub fn diagnostic_severity(kind: DiagnosticKind) -> DiagnosticSeverity {
    match kind {
        DiagnosticKind::Error => DiagnosticSeverity::ERROR,
        DiagnosticKind::Warning => DiagnosticSeverity::WARNING,
        DiagnosticKind::Hint => DiagnosticSeverity::HINT,
    }
}
