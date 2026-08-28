#![allow(unused_crate_dependencies)]

use varn_lsp::features::inlay_hints::build_inlay_hints;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_no_inlay_hints_when_explicit_type_present() {
    let source = r#"
class SortedPair<T> {
    first: T;
    second: T;

    constructor(a: T, b: T) {
        this.first = a;
        this.second = b;
    }

    swap(): SortedPair<T> {
        return new SortedPair<T>(this.second, this.first);
    }

    toArray(): T[] {
        return [this.first, this.second];
    }
}
"#;
    let uri = "file:///test/sorted_pair.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    let hints = build_inlay_hints(&state);
    // There should be NO return type hints on swap() or toArray() because they have explicit return types!
    assert!(
        hints.is_empty(),
        "Expected 0 inlay hints for fully explicitly typed class, but got: {:?}",
        hints
    );
}

#[test]
fn test_inlay_hints_generated_when_type_inferred() {
    let source = r#"
const num = 42;
const greeting = "hello";
"#;
    let uri = "file:///test/inferred.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    let hints = build_inlay_hints(&state);
    // Should have type hint for `num` (: int) and `greeting` (: str)
    assert_eq!(
        hints.len(),
        2,
        "Expected 2 inlay hints for inferred consts, got: {:?}",
        hints
    );
    if let tower_lsp::lsp_types::InlayHintLabel::String(s) = &hints[0].label {
        assert_eq!(s, ": int");
    } else {
        panic!("Expected String label");
    }
    if let tower_lsp::lsp_types::InlayHintLabel::String(s) = &hints[1].label {
        assert_eq!(s, ": str");
    } else {
        panic!("Expected String label");
    }
}
