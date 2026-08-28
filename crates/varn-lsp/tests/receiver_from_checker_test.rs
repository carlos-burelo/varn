#![allow(unused_crate_dependencies)]

use varn_lsp::features::hover::build_hover;
use varn_lsp::pipeline::run_pipeline;
use tower_lsp::lsp_types::HoverContents;

#[test]
fn test_receiver_method_and_property_hover() {
    let source = r#"
class Calculator {
    value: int;

    get_value(): int {
        return this.value;
    }
}

const c = new Calculator();
const res = c.get_value();
"#;
    let uri = "file:///test/main.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    // Hover over get_value call on line 10
    // "const res = c.get_value();" -> line 10, col 16
    let hover = build_hover(&state, 10, 16);
    assert!(hover.is_some(), "Hover over c.get_value() should return Some");
    let hover_val = hover.unwrap();
    if let HoverContents::Markup(markup) = hover_val.contents {
        assert!(markup.value.contains("get_value"), "Hover should describe get_value method: {}", markup.value);
    } else {
        panic!("Expected markup hover content");
    }
}

#[test]
fn test_receiver_primitive_methods() {
    let source = r#"
const text = "  hello world  ";
const trimmed = text.trim();
"#;
    let uri = "file:///test/main.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    // Hover over text.trim() on line 2, col 23
    let hover = build_hover(&state, 2, 23);
    assert!(hover.is_some(), "Hover over trim() on str should succeed");
}
