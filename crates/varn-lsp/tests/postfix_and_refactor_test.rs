#![allow(unused_crate_dependencies)]

use tower_lsp::lsp_types::{FormattingOptions, Position, Range, Url};
use varn_lsp::features::code_action::extract_function::generate_extract_function_action;
use varn_lsp::features::code_action::extract_variable::generate_extract_variable_action;
use varn_lsp::features::code_action::generate_members::generate_class_member_actions;
use varn_lsp::features::completion::postfix::build_postfix_completions;
use varn_lsp::features::formatting::build_formatting;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_postfix_completions() {
    let src = "let x = user.name.\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    let items = build_postfix_completions(&state, 0, 18);
    assert!(!items.is_empty(), "Should generate postfix completion items");

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"let"), "Should have .let postfix");
    assert!(labels.contains(&"const"), "Should have .const postfix");
    assert!(labels.contains(&"if"), "Should have .if postfix");
    assert!(labels.contains(&"while"), "Should have .while postfix");
    assert!(labels.contains(&"for"), "Should have .for postfix");
    assert!(labels.contains(&"match"), "Should have .match postfix");
    assert!(labels.contains(&"dbg"), "Should have .dbg postfix");
}

#[test]
fn test_extract_variable_action() {
    let src = "let total = price * 0.15;\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    let uri = Url::parse("file:///test.vn").unwrap();
    let range = Range {
        start: Position { line: 0, character: 12 },
        end: Position { line: 0, character: 24 },
    };
    let action = generate_extract_variable_action(&state, &uri, range);
    assert!(action.is_some(), "Should generate extract variable action");
}

#[test]
fn test_extract_function_action() {
    let src = "fn main() {\n    let a = 10;\n    let b = 20;\n    let c = a + b;\n}\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    let uri = Url::parse("file:///test.vn").unwrap();
    let range = Range {
        start: Position { line: 1, character: 4 },
        end: Position { line: 3, character: 18 },
    };
    let action = generate_extract_function_action(&state, &uri, range);
    assert!(action.is_some(), "Should generate extract function action");
}

#[test]
fn test_generate_class_members() {
    let src = "class User {\n    name: str;\n    age: int;\n}\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    let uri = Url::parse("file:///test.vn").unwrap();
    let actions = generate_class_member_actions(&state, &uri, 1);
    assert!(!actions.is_empty(), "Should generate constructor and getter/setter actions");
}

#[test]
fn test_formatting_engine() {
    let unformatted = "fn test() {\nlet x = 10;\nif x > 0 {\nprintln(x);\n}\n}\n";
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        properties: Default::default(),
        trim_trailing_whitespace: Some(true),
        insert_final_newline: Some(true),
        trim_final_newlines: Some(true),
    };
    let edits = build_formatting(unformatted, options);
    assert!(edits.is_some(), "Should produce indentation edits");
    let edits_list = edits.unwrap();
    assert!(!edits_list.is_empty(), "Should have edits for unindented lines");
}

#[test]
fn test_reflection_colon_colon_completion() {
    let src = "class SampleUser {\n    name: str;\n    age: int;\n}\nconst fields = SampleUser::;\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    let receiver = varn_lsp::features::completion::reflection::colon_colon_receiver(&state, 4, 27, Some(":"));
    assert_eq!(receiver.as_deref(), Some("SampleUser"));

    let items = varn_lsp::features::completion::reflection::build_reflection_completions(&state, "SampleUser");
    assert!(!items.is_empty(), "Should generate reflection items");

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"name"), "Should have ::name");
    assert!(labels.contains(&"fields"), "Should have ::fields");
    assert!(labels.contains(&"methods"), "Should have ::methods");
    assert!(labels.contains(&"type"), "Should have ::type");
    assert!(labels.contains(&"class"), "Should have ::class");
    assert!(labels.contains(&"keys"), "Should have ::keys");
    assert!(labels.contains(&"values"), "Should have ::values");
    assert!(labels.contains(&"entries"), "Should have ::entries");
    assert!(labels.contains(&"hasOwn"), "Should have ::hasOwn");
}

#[test]
fn test_root_scope_isolation_from_unreachable_type_parameters() {
    let src = "function compose<A, B, C>(f: (a: B) => C, g: (b: A) => B): (c: A) => C {\n    return (x: A) => f(g(x));\n}\nconst double: (n: int) => int = (n: int) => n * 2;\n\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    
    // At line 4 (top-level, root scope)
    let root_items = varn_lsp::features::completion::build_completions(&state, 4, 0);
    let root_labels: Vec<&str> = root_items.iter().map(|i| i.label.as_str()).collect();

    assert!(root_labels.contains(&"compose"), "Root should contain top-level function compose");
    assert!(root_labels.contains(&"double"), "Root should contain top-level const double");
    
    // MUST NOT contain type parameters A, B, C or inner function parameters x, f, g
    assert!(!root_labels.contains(&"A"), "Root scope must NOT contain unreachable type parameter A");
    assert!(!root_labels.contains(&"B"), "Root scope must NOT contain unreachable type parameter B");
    assert!(!root_labels.contains(&"C"), "Root scope must NOT contain unreachable type parameter C");
    assert!(!root_labels.contains(&"x"), "Root scope must NOT contain inner closure variable x");
}

#[test]
fn test_get_cfg_graph_json() {
    let src = "function max(a: int, b: int): int {\n    if a > b {\n        return a;\n    } else {\n        return b;\n    }\n}\n";
    let state = run_pipeline(src.to_string(), "file:///test.vn".to_string());
    
    let cfg_json = varn_lsp::features::compiler_inspect::compile_and_get_cfg_json(&state);
    assert!(cfg_json.is_ok(), "CFG compilation must succeed");
    
    let json_val = cfg_json.unwrap();
    assert!(json_val.get("functions").is_some(), "Must have functions array");
    assert!(json_val.get("bytecode").is_some(), "Must have bytecode disassembly");
}
