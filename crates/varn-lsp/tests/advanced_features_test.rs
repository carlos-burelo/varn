#![allow(unused_crate_dependencies)]

use tower_lsp::lsp_types::{Position, Url};
use varn_lsp::features::call_hierarchy::prepare_call_hierarchy;
use varn_lsp::features::code_action::build_code_action;
use varn_lsp::features::compiler_inspect::compile_and_disassemble;
use varn_lsp::features::inlay_hints::build_inlay_hints;
use varn_lsp::features::selection_range::build_selection_ranges;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_match_arms_code_action() {
    let source = r#"
enum Color {
    Red,
    Green,
    Blue,
}

function describe(c: Color) {
    match c {
        Color.Red => {}
    }
}
"#;
    let uri = "file:///test/match.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());
    let url = Url::parse(uri).unwrap();

    let params = tower_lsp::lsp_types::CodeActionParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri: url },
        range: tower_lsp::lsp_types::Range {
            start: Position {
                line: 8,
                character: 4,
            },
            end: Position {
                line: 8,
                character: 4,
            },
        },
        context: tower_lsp::lsp_types::CodeActionContext {
            diagnostics: Vec::new(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = build_code_action(params, Some(&state), None);
    assert!(actions.is_some());
    let action_list = actions.unwrap();
    let has_fill_arms = action_list.iter().any(|a| match a {
        tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) => {
            ca.title.contains("Fill missing match arms")
        }
        _ => false,
    });
    assert!(has_fill_arms, "Expected 'Fill missing match arms' action");
}

#[test]
fn test_interface_impl_code_action() {
    let source = r#"
interface Greeter {
    greet(name: string): string
    farewell(name: string): string
}

class EnglishGreeter implements Greeter {
}
"#;
    let uri = "file:///test/iface.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());
    let url = Url::parse(uri).unwrap();

    let params = tower_lsp::lsp_types::CodeActionParams {
        text_document: tower_lsp::lsp_types::TextDocumentIdentifier { uri: url },
        range: tower_lsp::lsp_types::Range {
            start: Position {
                line: 6,
                character: 6,
            },
            end: Position {
                line: 6,
                character: 6,
            },
        },
        context: tower_lsp::lsp_types::CodeActionContext {
            diagnostics: Vec::new(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = build_code_action(params, Some(&state), None);
    assert!(actions.is_some());
    let action_list = actions.unwrap();
    let has_impl_members = action_list.iter().any(|a| match a {
        tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(ca) => {
            ca.title.contains("Implement missing members of interface")
        }
        _ => false,
    });
    assert!(
        has_impl_members,
        "Expected 'Implement missing members' action"
    );
}

#[test]
fn test_inlay_hints_and_selection_range() {
    let source = r#"
function add(a: int, b: int): int {
    let sum = a + b;
    return sum;
}

function main() {
    let res = add(10, 20);
}
"#;
    let uri = "file:///test/inlay.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());

    // Inlay hints test
    let hints = build_inlay_hints(&state);
    assert!(!hints.is_empty(), "Expected inlay hints for let bindings");

    // Selection range test
    let pos = Position {
        line: 2,
        character: 16,
    };
    let sel_ranges = build_selection_ranges(&state, &[pos]);
    assert_eq!(sel_ranges.len(), 1);
    assert!(sel_ranges[0].parent.is_some());
}

#[test]
fn test_call_hierarchy_and_bytecode_disassemble() {
    let source = r#"
function helper(x: int): int {
    return x * 2;
}

function main() {
    let val = helper(21);
}
"#;
    let uri = "file:///test/call.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());

    // Call hierarchy prepare test
    let hierarchy = prepare_call_hierarchy(&state, 1, 9);
    assert!(hierarchy.is_some());
    let items = hierarchy.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "helper");

    // Bytecode disassemble test
    let bytecode = compile_and_disassemble(&state);
    assert!(bytecode.is_ok());
    let disasm = bytecode.unwrap();
    assert!(disasm.contains("=== Function 'main'"));
    assert!(disasm.contains("=== Function 'helper'"));
}
