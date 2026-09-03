#![allow(unused_crate_dependencies)]

use varn_lsp::features::completion::build_completion_response;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_incomplete_variable_declaration() {
    // Half-typed variable declaration with missing initializer
    let source = "const x = ";
    let uri = "file:///test/incomplete.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    // Should not crash and should parse
    assert!(state.ast.is_some());
}

#[test]
fn test_dot_completion_on_incomplete_code() {
    let source = r#"
class Person {
    name: str;
    age: int;
}

const p = new Person();
p.
"#;
    let uri = "file:///test/person.vn".to_string();
    let state = run_pipeline(source.to_string(), uri);

    // Trigger completion right after 'p.' on line 7, col 2
    let (completions, _) = build_completion_response(
        &state,
        7,
        2,
        Some("."),
        "trigger_character".to_string(),
        None,
    );
    assert!(
        completions.is_some(),
        "Completion after dot on 'p.' should return candidate members"
    );
    let items = completions.unwrap();
    let names: Vec<String> = match items {
        tower_lsp::lsp_types::CompletionResponse::Array(arr) => {
            arr.into_iter().map(|i| i.label).collect()
        }
        tower_lsp::lsp_types::CompletionResponse::List(list) => {
            list.items.into_iter().map(|i| i.label).collect()
        }
    };

    assert!(
        names.contains(&"name".to_string()),
        "Completion should suggest 'name'"
    );
    assert!(
        names.contains(&"age".to_string()),
        "Completion should suggest 'age'"
    );
}
