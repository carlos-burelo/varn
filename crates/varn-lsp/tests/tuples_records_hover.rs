#![allow(unused_crate_dependencies)] // per-target lint: a test needs only a slice of the crate deps

use tower_lsp::lsp_types::{HoverContents, MarkupContent};
use varn_lsp::workspace::Workspace;

#[test]
fn test_tuple_and_record_hovers_format() {
    let ws = Workspace::new();
    let uri = "file:///test_tuples.vn".to_string();
    let source = r#"
let t1 = #[10, "hello", true]
let r1 = #{ name: "Varn", version: 1 }
let elem = t1[0]
let field = r1.name
"#;

    ws.update_file(uri.clone(), source.to_string());
    let state = ws.get(&uri).expect("analysis state should exist");

    // Test hover on t1 (line 1, col 5)
    let hover_t1 = varn_lsp::features::hover::build_hover(&state, 1, 5);
    assert!(hover_t1.is_some(), "Hover on t1 should exist");
    if let Some(h) = hover_t1 {
        if let HoverContents::Markup(MarkupContent { value, .. }) = h.contents {
            assert!(
                value.contains("```varn\n"),
                "Hover markdown must start with lowercase ```varn block, got:\n{}",
                value
            );
            assert!(
                value.contains("#[int, str, bool]"),
                "Hover on tuple t1 should contain tuple type #[int, str, bool], got:\n{}",
                value
            );
        } else {
            panic!("Expected MarkupContent hover");
        }
    }

    // Test hover on r1 (line 2, col 5)
    let hover_r1 = varn_lsp::features::hover::build_hover(&state, 2, 5);
    assert!(hover_r1.is_some(), "Hover on r1 should exist");
    if let Some(h) = hover_r1 {
        if let HoverContents::Markup(MarkupContent { value, .. }) = h.contents {
            assert!(
                value.contains("```varn\n"),
                "Hover markdown must start with lowercase ```varn block, got:\n{}",
                value
            );
            assert!(
                value.contains("name: str"),
                "Hover on record r1 should contain record field name: str, got:\n{}",
                value
            );
        } else {
            panic!("Expected MarkupContent hover");
        }
    }
}
