pub mod auto_import;
pub mod extract_function;
pub mod extract_variable;
pub mod generate_members;
pub mod interface_impl;
pub mod match_arms;
pub mod organize_imports;

use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, Position, Range, TextEdit,
    WorkspaceEdit,
};

use crate::document::DocumentState;
use crate::index::ProjectIndex;

pub fn build_code_action(
    params: CodeActionParams,
    state: Option<&DocumentState>,
    index: Option<&ProjectIndex>,
) -> Option<Vec<CodeActionOrCommand>> {
    let mut actions = Vec::new();
    let uri = &params.text_document.uri;
    let cursor_line = params.range.start.line;
    let cursor_col = params.range.start.character;

    // 1. Diagnostic suggestions and Quickfixes
    for diag in &params.context.diagnostics {
        if let Some(data) = &diag.data {
            if let Some(suggestions) = data.get("suggestions").and_then(|s| s.as_array()) {
                for sug in suggestions {
                    let msg = sug
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Fix issue");
                    let mut action = CodeAction {
                        title: msg.to_string(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: None,
                        command: None,
                        is_preferred: Some(true),
                        disabled: None,
                        data: None,
                    };

                    if let Some(repl) = sug.get("replacement").and_then(|r| r.as_str()) {
                        let mut edits = Vec::new();

                        let range = if let Some(r) = sug.get("range") {
                            let start_line = r
                                .get("start")
                                .and_then(|s| s.get("line"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(0) as u32;
                            let start_col = r
                                .get("start")
                                .and_then(|s| s.get("character"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(0) as u32;
                            let end_line = r
                                .get("end")
                                .and_then(|s| s.get("line"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(0) as u32;
                            let end_col = r
                                .get("end")
                                .and_then(|s| s.get("character"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(0) as u32;

                            Range {
                                start: Position {
                                    line: start_line.saturating_sub(1),
                                    character: start_col,
                                },
                                end: Position {
                                    line: end_line.saturating_sub(1),
                                    character: end_col,
                                },
                            }
                        } else {
                            diag.range
                        };

                        edits.push(TextEdit {
                            range,
                            new_text: repl.to_string(),
                        });

                        let mut changes = HashMap::new();
                        changes.insert(uri.clone(), edits);

                        action.edit = Some(WorkspaceEdit {
                            changes: Some(changes),
                            document_changes: None,
                            change_annotations: None,
                        });
                    }

                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }

        // Auto-Import suggestion for undefined variables
        if let Some(st) = state {
            let auto_imports = auto_import::generate_auto_imports_action(st, index, uri, diag);
            actions.extend(auto_imports);
        }
    }

    // 2. Semantic Code Actions (Match arms, Interface Implementation, Organize Imports)
    if let Some(st) = state {
        if let Some(match_action) =
            match_arms::generate_match_arms_action(st, uri, cursor_line, cursor_col)
        {
            actions.push(match_action);
        }

        if let Some(iface_action) =
            interface_impl::generate_interface_impl_action(st, index, uri, cursor_line, cursor_col)
        {
            actions.push(iface_action);
        }

        if let Some(organize_action) =
            organize_imports::generate_organize_imports_action(st, uri)
        {
            actions.push(organize_action);
        }

        if let Some(extract_var) =
            extract_variable::generate_extract_variable_action(st, uri, params.range)
        {
            actions.push(extract_var);
        }

        if let Some(extract_fn) =
            extract_function::generate_extract_function_action(st, uri, params.range)
        {
            actions.push(extract_fn);
        }

        let class_actions = generate_members::generate_class_member_actions(st, uri, cursor_line);
        actions.extend(class_actions);
    }

    if actions.is_empty() {
        None
    } else {
        Some(actions)
    }
}
