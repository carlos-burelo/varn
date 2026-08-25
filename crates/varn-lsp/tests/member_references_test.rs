#![allow(unused_crate_dependencies)]

//! References and rename must work on class members, not only on top-level
//! declarations.
//!
//! They did not. The target symbol was keyed by `symbol_global_key_for_id`,
//! which yields the `u:`/`m:` form, while every candidate token was keyed by
//! `token_global_key`, which yields `member:{type}:{name}` when the token is a
//! class member. Two shapes for one question: for a member they could never
//! compare equal, so "find all references" on a field returned nothing and
//! "rename" edited nothing — silently, with no error to notice.
//!
//! It went unseen because on a top-level function both shapes coincide, and
//! that is what anyone testing by hand would try first. Hence the pairing in
//! every test here: the member case *and* the function case, so a fix that
//! trades one for the other cannot pass.

use varn_lsp::features::references::build_references;
use varn_lsp::features::rename::build_rename;
use varn_lsp::workspace::Workspace;

const SRC: &str = "class Widget {\n\
                   \x20   value: int = 0\n\
                   \x20   fn bump(): int { return this.value + 1 }\n\
                   }\n\
                   const w = new Widget()\n\
                   const a = w.value\n\
                   const b = w.value + 1\n\
                   function top(): int { return 7 }\n\
                   const c = top()\n\
                   const d = top()\n";

const URI: &str = "file:///test/members.vn";

fn workspace() -> Workspace {
    let ws = Workspace::new();
    ws.update_file(URI.to_owned(), SRC.to_owned());
    ws
}

/// The declaration, `this.value`, and both `w.value` reads.
#[test]
fn references_finds_every_use_of_a_class_member() {
    let ws = workspace();
    let state = ws.get(URI).expect("the document must be analysed");

    let locs = build_references(&state, &ws, 1, 4)
        .expect("a class member must have references; returning none is the bug this pins");

    let mut lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        vec![1, 2, 5, 6],
        "expected the declaration, `this.value`, and both `w.value` reads"
    );
}

/// The case that always worked, kept alongside so a fix cannot trade one for
/// the other.
#[test]
fn references_still_finds_every_use_of_a_top_level_function() {
    let ws = workspace();
    let state = ws.get(URI).expect("the document must be analysed");

    let locs = build_references(&state, &ws, 7, 9).expect("a top-level function must have refs");
    let mut lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
    lines.sort_unstable();
    assert_eq!(lines, vec![7, 8, 9]);
}

#[test]
fn rename_rewrites_every_use_of_a_class_member() {
    let ws = workspace();
    let state = ws.get(URI).expect("the document must be analysed");

    let edit = build_rename(&state, &ws, None, 1, 4, "amount".to_owned())
        .expect("renaming a class member must produce edits; producing none is the bug");

    let count: usize = edit
        .changes
        .as_ref()
        .map(|c| c.values().map(Vec::len).sum())
        .unwrap_or(0);
    assert_eq!(
        count, 4,
        "every use of the member must be rewritten, not just its declaration"
    );
}
