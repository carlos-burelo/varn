//! Mapping between LSP positions and byte offsets.
//!
//! One function, because there is one question: where in the source does a
//! position point? Cursor positions on a request and edit ranges on a keystroke
//! are the same question asked by different handlers, and answering it twice
//! is how the two answers drift apart.
//!
//! **Positions are UTF-16 code units.** That is the LSP default and the only
//! encoding this server advertises, so the column walk goes through
//! `char::len_utf16` — counting `char`s instead is correct for everything in
//! the BMP and off by one per astral character (an emoji in a string literal
//! ahead of the cursor), which is exactly the kind of drift that shows up as
//! "hover works except in that one file".
//!
//! Lines are split on `\n`; the column walk also stops at a `\r`, so a position
//! clamped past the end of a CRLF line lands before the terminator rather than
//! inside it.

use tower_lsp::lsp_types::Position;

/// Byte offset of `pos` in `text`, clamped to the document.
///
/// An out-of-range position is not something to reject: a client racing an edit
/// against a text it has already changed will send one, and the repair is to
/// clamp — to the end of the line, or of the document — rather than to panic on
/// a slice boundary that is not a char boundary.
pub fn byte_offset(text: &str, pos: Position) -> usize {
    let line_start = line_start(text, pos.line);
    column_offset(&text[line_start..], pos.character) + line_start
}

/// Byte offset where line `line` starts, or the end of `text` if it has fewer
/// lines than that.
fn line_start(text: &str, line: u32) -> usize {
    if line == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line {
                return i + 1;
            }
        }
    }
    text.len()
}

/// Byte offset, within a slice that starts at a line, of the `character`th
/// UTF-16 code unit of that line.
fn column_offset(line: &str, character: u32) -> usize {
    let mut units = 0u32;
    for (i, ch) in line.char_indices() {
        if units >= character || ch == '\n' || ch == '\r' {
            return i;
        }
        units += ch.len_utf16() as u32;
    }
    line.len()
}
