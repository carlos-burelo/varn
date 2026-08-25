#![allow(unused_crate_dependencies)]

//! The receiver of a member access comes from the checker.
//!
//! It used to come from a hand-written walk of the whole AST, re-run from the
//! root on every hover and goto, whose `match` needed a new arm for each
//! `ExprKind` and already missed several. The checker records the answer, typed,
//! in `member_resolutions[offset].receiver_ty`.
//!
//! These pin the cases the walk got wrong or would have had to grow an arm for.
//! A token-before-the-dot fallback cannot answer any of them: in each, the token
//! preceding the dot is `)` or `]`, not a name.

use varn_lsp::features::hover::build_hover;
use varn_lsp::pipeline::run_pipeline;

fn analyze(source: &str) -> varn_lsp::document::DocumentAnalysis {
    run_pipeline(source.to_string(), "file:///test/receiver.vn".to_string())
}

fn hover_text(state: &varn_lsp::document::DocumentAnalysis, line: u32, col: u32) -> String {
    match build_hover(state, line, col) {
        Some(h) => match h.contents {
            tower_lsp::lsp_types::HoverContents::Markup(m) => m.value,
            _ => String::new(),
        },
        None => String::new(),
    }
}

/// Member on the result of a call. The token before the dot is `)`.
#[test]
fn receiver_is_a_call_result() {
    let state = analyze("function make(): str {\n  return \"x\"\n}\nconst n = make().length\n");
    let text = hover_text(&state, 3, 18);
    assert!(
        text.contains("int"),
        "`make().length` must resolve through the call's return type; got {text:?}"
    );
}

/// Member on an index expression. The token before the dot is `]`.
#[test]
fn receiver_is_an_index_result() {
    let state = analyze("const xs: str[] = [\"a\"]\nconst n = xs[0].length\n");
    let text = hover_text(&state, 1, 17);
    assert!(
        text.contains("int"),
        "`xs[0].length` must resolve through the element type; got {text:?}"
    );
}

/// Chained members. The receiver of the second dot is the first member's type,
/// which no token adjacent to that dot names.
#[test]
fn receiver_is_an_earlier_member_in_a_chain() {
    let state = analyze("const s: str = \"hola\"\nconst n = s.trim().length\n");
    let text = hover_text(&state, 1, 21);
    assert!(
        text.contains("int"),
        "`s.trim().length` must resolve through `trim`'s return type; got {text:?}"
    );
}

/// A member the checker resolved carries its receiver; hover reports the member,
/// not the receiver's own name.
#[test]
fn a_resolved_member_reports_its_own_type() {
    let state = analyze("const s: str = \"hola\"\nconst n = s.length\n");
    let text = hover_text(&state, 1, 13);
    assert!(
        text.contains("length") && text.contains("int"),
        "hover on `length` must name the member and its type; got {text:?}"
    );
}
