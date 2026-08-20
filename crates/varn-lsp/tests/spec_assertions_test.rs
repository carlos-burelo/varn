#![allow(unused_crate_dependencies)]

use varn_lsp::features::hover::build_hover;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_primitive_and_intrinsic_hovers() {
    let source = r#"
function demo(c: char, d: decimal, b: bigint, t: Task<int>) {
    let flag = true;
    let n = null;
    let opt = Option.None;
}
"#;
    let uri = "file:///test/intrinsics.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());

    // 1. Hover on 'char'
    let hover_char = build_hover(&state, 1, 18).expect("Hover on char should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_char.contents {
        assert!(mc.value.contains("type char"), "Expected 'type char' in hover: {}", mc.value);
        assert!(mc.value.contains("Unicode"), "Expected Unicode description in hover");
    }

    // 2. Hover on 'decimal'
    let hover_dec = build_hover(&state, 1, 27).expect("Hover on decimal should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_dec.contents {
        assert!(mc.value.contains("type decimal"), "Expected 'type decimal' in hover: {}", mc.value);
        assert!(mc.value.contains("128 bits"), "Expected 128-bit description in hover");
    }

    // 3. Hover on 'bigint'
    let hover_bigint = build_hover(&state, 1, 39).expect("Hover on bigint should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_bigint.contents {
        assert!(mc.value.contains("type bigint"), "Expected 'type bigint' in hover: {}", mc.value);
    }

    // 4. Hover on 'Task'
    let hover_task = build_hover(&state, 1, 49).expect("Hover on Task should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_task.contents {
        assert!(mc.value.contains("type Task<T>"), "Expected 'type Task<T>' in hover: {}", mc.value);
    }

    // 5. Hover on 'true'
    let hover_true = build_hover(&state, 2, 16).expect("Hover on true should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_true.contents {
        assert!(mc.value.contains("true: bool"), "Expected 'true: bool' in hover: {}", mc.value);
    }

    // 6. Hover on 'null'
    let hover_null = build_hover(&state, 3, 13).expect("Hover on null should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_null.contents {
        assert!(mc.value.contains("null"), "Expected 'null' in hover: {}", mc.value);
    }
}

#[test]
fn test_decorator_and_control_flow_hovers() {
    let source = r#"
@inline
function fastAdd(a: int, b: int): int {
    return a + b;
}

function process(x: int): str {
    return match (x) {
        0 => "zero",
        _ => "other"
    };
}
"#;
    let uri = "file:///test/control.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());

    // 1. Hover on '@inline'
    let hover_inline = build_hover(&state, 1, 2).expect("Hover on @inline should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_inline.contents {
        assert!(mc.value.contains("@inline"), "Expected '@inline' in hover: {}", mc.value);
        assert!(mc.value.contains("compilador"), "Expected compiler description in hover");
    }

    // 2. Hover on 'match'
    let hover_match = build_hover(&state, 7, 12).expect("Hover on match should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_match.contents {
        assert!(mc.value.contains("match"), "Expected 'match' in hover: {}", mc.value);
        assert!(mc.value.contains("pattern matching"), "Expected pattern matching description in hover");
    }
}
