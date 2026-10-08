use tower_lsp_f::lsp_types::TextDocumentContentChangeEvent;

use crate::document::position::byte_offset;

pub fn apply_change(source: &mut String, change: TextDocumentContentChangeEvent) {
    match change {
        TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(whole) => {
            *source = whole.text;
        }
        TextDocumentContentChangeEvent::TextDocumentContentChangePartial(partial) => {
            let start = byte_offset(source, partial.range.start);
            let end = byte_offset(source, partial.range.end).max(start);
            source.replace_range(start..end, &partial.text);
        }
    }
}

pub fn apply_changes(
    source: &mut String,
    changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
) {
    for change in changes {
        apply_change(source, change);
    }
}
