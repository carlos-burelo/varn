






use tower_lsp::lsp_types::TextDocumentContentChangeEvent;

use crate::document::position::byte_offset;








pub fn apply_change(source: &mut String, change: TextDocumentContentChangeEvent) {
    let Some(range) = change.range else {
        *source = change.text;
        return;
    };

    let start = byte_offset(source, range.start);
    
    
    let end = byte_offset(source, range.end).max(start);
    source.replace_range(start..end, &change.text);
}






pub fn apply_changes(
    source: &mut String,
    changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
) {
    for change in changes {
        apply_change(source, change);
    }
}
