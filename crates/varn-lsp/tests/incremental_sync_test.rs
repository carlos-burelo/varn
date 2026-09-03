#![allow(unused_crate_dependencies)]

use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent};
use varn_lsp::backend::sync::edits::apply_change;
use varn_lsp::document::position::byte_offset;

#[test]
fn test_incremental_edit_application() {
    let mut doc = "function hello() {\n    return 42;\n}\n".to_string();

    // Replace `42` with `100` on line 1 (0-indexed), cols 11..13
    let change = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position {
                line: 1,
                character: 11,
            },
            end: Position {
                line: 1,
                character: 13,
            },
        }),
        range_length: None,
        text: "100".to_string(),
    };

    apply_change(&mut doc, change);
    assert_eq!(doc, "function hello() {\n    return 100;\n}\n");
}

#[test]
fn test_position_with_utf16_astral_characters() {
    // A string with an emoji (4 bytes in UTF-8, 2 code units in UTF-16)
    let source = "const emoji = \"😀\";\nconst next = 1;\n";

    // Start of line 1 (second line)
    let offset_line1 = byte_offset(
        source,
        Position {
            line: 1,
            character: 0,
        },
    );
    let expected = "const emoji = \"😀\";\n".len();
    assert_eq!(offset_line1, expected);
}
