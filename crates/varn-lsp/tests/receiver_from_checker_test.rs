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

/// Builtin members report what the standard library declares.
///
/// A hand-written table of these signatures used to answer *before* the
/// checker, so it won for every type it covered — and it was a transcription of
/// `std/` kept in sync by hand, indexed by string surgery on the receiver's
/// printed type (`split('<')`, `ends_with("[]")`). It could not express a
/// generic callback, so `map` came back flattened.
///
/// The assertions below are on the fidelity the table could not reach: the
/// callback's own parameter types, and a generic receiver preserved as
/// `Map<int>` rather than rebuilt from its printed form.
#[test]
fn builtin_member_signatures_come_from_the_stdlib() {
    let state = analyze(
        "const a: int[] = [1, 2, 3]\n\
         const j = a.map((x: int): int => x * 2)\n\
         const mp: Map<int> = new Map<int>()\n\
         const vs = mp.values()\n",
    );

    let map_hover = hover_text(&state, 1, 12);
    assert!(
        map_hover.contains("item: int") && map_hover.contains("array: int[]"),
        "`map` must carry the callback's declared parameter types; got {map_hover:?}"
    );

    let values_hover = hover_text(&state, 3, 14);
    assert!(
        values_hover.contains("Map<int>") && values_hover.contains("int[]"),
        "`values` must report the generic receiver and element type from std; \
         got {values_hover:?}"
    );
}
