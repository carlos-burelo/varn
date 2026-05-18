use tower_lsp::lsp_types::{Hover, HoverContents, LanguageString, MarkedString};

use crate::document::SymbolRecord;

use super::format::format_signature;

pub fn symbol_hover(sym: &SymbolRecord) -> Hover {
    let mut items = vec![MarkedString::LanguageString(LanguageString {
        language: "Varn".into(),
        value: format_signature(sym),
    })];

    if let Some(raw) = &sym.doc {
        let parsed = varn_core::DocComment::parse(raw);
        let md = parsed.to_markdown();
        if !md.is_empty() {
            items.push(MarkedString::String(md));
        }
    }

    Hover {
        contents: HoverContents::Array(items),
        range: None,
    }
}
