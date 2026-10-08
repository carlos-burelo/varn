#![allow(unused_crate_dependencies)]

use varn_lsp::features::completion::build_completion_response;
use varn_lsp::pipeline::run_pipeline;

fn test_resolver() -> std::sync::Arc<varn_checker::module_resolver::DiskResolver> {
    std::sync::Arc::new(varn_checker::module_resolver::DiskResolver::new())
}

#[test]
fn test_incomplete_variable_declaration() {
    let source = "const x = ";
    let uri = "file:///test/incomplete.vn".to_string();
    let state = run_pipeline(source.to_string(), uri, test_resolver());

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
    let state = run_pipeline(source.to_string(), uri, test_resolver());

    let (completions, _) = build_completion_response(
        &state,
        state.resolver.as_ref(),
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
        tower_lsp_f::lsp_types::CompletionResponse::CompletionItemList(arr) => {
            arr.into_iter().map(|i| i.label).collect()
        }
        tower_lsp_f::lsp_types::CompletionResponse::CompletionList(list) => {
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
