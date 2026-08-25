//! Applying `textDocument/didChange` content changes to a document's text.
//!
//! Pure text in, pure text out: no server state, so the rules below are
//! testable without a running server — which is the point, because an
//! off-by-one here corrupts the buffer the checker sees and every feature
//! answers from a file the user never wrote.

use tower_lsp::lsp_types::TextDocumentContentChangeEvent;

use crate::document::position::byte_offset;

/// Apply one content change in place.
///
/// A change with no range replaces the whole document: that is what a FULL-sync
/// client sends, and what `didSave` sends when it carries text. Both stay
/// supported because the encoding of an edit is the client's choice, not the
/// server's — advertising incremental sync does not stop a client from sending
/// a whole document.
pub fn apply_change(source: &mut String, change: TextDocumentContentChangeEvent) {
    let Some(range) = change.range else {
        *source = change.text;
        return;
    };

    let start = byte_offset(source, range.start);
    // A reversed range is malformed, and clamping it to an empty span inserts
    // where the edit began instead of panicking on `start > end`.
    let end = byte_offset(source, range.end).max(start);
    source.replace_range(start..end, &change.text);
}

/// Apply a batch, in order.
///
/// Order is load-bearing: each change is stated against the text left by the
/// one before it, so a batch applied out of order does not merely reorder the
/// result, it lands the later ranges on the wrong offsets.
pub fn apply_changes(
    source: &mut String,
    changes: impl IntoIterator<Item = TextDocumentContentChangeEvent>,
) {
    for change in changes {
        apply_change(source, change);
    }
}
