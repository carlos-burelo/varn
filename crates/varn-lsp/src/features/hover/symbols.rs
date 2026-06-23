use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::document::SymbolRecord;

use super::format::format_signature;

pub fn symbol_hover(sym: &SymbolRecord) -> Hover {
    // A Markdown code fence (not the deprecated MarkedString) so the client
    // syntax-highlights the signature via the `Varn` TextMate grammar.
    let mut value = format!("```Varn\n{}\n```", format_signature(sym));

    if let Some(raw) = &sym.doc {
        let parsed = varn_core::DocComment::parse(raw);
        let md = parsed.to_markdown();
        if !md.is_empty() {
            value.push_str("\n\n");
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
