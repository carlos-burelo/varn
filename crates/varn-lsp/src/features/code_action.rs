use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, Position, Range, TextEdit,
    WorkspaceEdit,
};

pub fn build_code_action(params: CodeActionParams) -> Option<Vec<CodeActionOrCommand>> {
    let mut actions = Vec::new();
    let uri = params.text_document.uri;

    for diag in params.context.diagnostics {
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
    }

    if actions.is_empty() {
        None
    } else {
        Some(actions)
    }
}
