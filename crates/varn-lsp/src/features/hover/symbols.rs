use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::document::{DocumentState, SymbolView};

use super::format::format_signature;

pub fn symbol_hover(state: &DocumentState, sym: SymbolView<'_>) -> Hover {
    let mut value = format!("```varn\n{}\n```", format_signature(state, sym));

    if let Some(origin) = &sym.origin() {
        value.push_str(&format!("\n*(from {})*", origin));
    } else if sym.is_from_stdlib() {
        value.push_str("\n*(from standard library)*");
    }

    if let Some(raw) = &sym.doc() {
        let parsed = varn_core::DocComment::parse(raw);
        let md = parsed.to_markdown();
        if !md.is_empty() {
            value.push_str("\n***\n");
            value.push_str(&md);
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}
