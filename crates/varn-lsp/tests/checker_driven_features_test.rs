#![allow(unused_crate_dependencies)]

use tower_lsp_f::lsp_types::{CodeActionResponse, DocumentChange, Edit, Label, Uri};
use varn_lsp::features::code_action::match_arms::generate_match_arms_action;
use varn_lsp::features::inlay_hints::param_hints::build_parameter_hints;
use varn_lsp::pipeline::run_pipeline;

fn test_resolver() -> std::sync::Arc<varn_checker::module_resolver::DiskResolver> {
    std::sync::Arc::new(varn_checker::module_resolver::DiskResolver::new())
}

fn fill_arms_edit(source: &str, line: u32) -> Option<String> {
    let uri = "file:///test/match.vn".to_string();
    let state = run_pipeline(source.to_string(), uri.clone(), test_resolver());
    let parsed = Uri::parse(&uri).unwrap();
    match generate_match_arms_action(&state, &parsed, line, 0)? {
        CodeActionResponse::CodeAction(action) => {
            let document_changes = action.edit?.document_changes?;
            Some(
                document_changes
                    .into_iter()
                    .filter_map(|d| match d {
                        DocumentChange::TextDocumentEdit(t) => Some(t),
                        DocumentChange::CreateFile(_)
                        | DocumentChange::RenameFile(_)
                        | DocumentChange::DeleteFile(_) => None,
                    })
                    .flat_map(|t| t.edits)
                    .filter_map(|e| match e {
                        Edit::TextEdit(t) => Some(t.new_text),
                        Edit::AnnotatedTextEdit(_) | Edit::SnippetTextEdit(_) => None,
                    })
                    .collect(),
            )
        }
        CodeActionResponse::Command(_) => None,
    }
}

#[test]
fn match_arms_come_from_the_checkers_gap() {
    let source = r#"
enum Shade { Dark, Light, Dim }
const s = Shade.Dark;
const n = match (s) {
    Dark => 0
};
"#;
    let edit = fill_arms_edit(source, 4).expect("an action on a non-exhaustive match");
    assert!(edit.contains("Light =>"), "{edit}");
    assert!(edit.contains("Dim =>"), "{edit}");
    assert!(!edit.contains("Dark =>"), "{edit}");
}

#[test]
fn match_arms_bind_a_payload_variants_fields() {
    let source = r#"
enum Outcome {
    Good(value: int),
    Bad(error: str)
}
const o = Outcome.Good(1);
const n = match (o) {
    Good(v) => v
};
"#;
    let edit = fill_arms_edit(source, 7).expect("an action on a non-exhaustive match");
    assert!(edit.contains("Bad(_) =>"), "{edit}");
}

#[test]
fn no_match_arms_action_on_an_exhaustive_match() {
    let source = r#"
const b = true;
const n = match (b) {
    true => 1,
    false => 0
};
"#;
    assert_eq!(fill_arms_edit(source, 3), None);
}

#[test]
fn parameter_hints_reach_calls_in_initializers() {
    let source = r#"
function area(width: int, height: int): int {
    return width * height;
}
const a = area(2, 3);
"#;
    let state = run_pipeline(
        source.to_string(),
        "file:///test/hints.vn".to_string(),
        test_resolver(),
    );
    let labels: Vec<String> = build_parameter_hints(&state)
        .into_iter()
        .filter_map(|h| match h.label {
            Label::String(s) => Some(s),
            Label::InlayHintLabelPartList(_) => None,
        })
        .collect();
    assert_eq!(labels, vec!["width: ", "height: "]);
}
