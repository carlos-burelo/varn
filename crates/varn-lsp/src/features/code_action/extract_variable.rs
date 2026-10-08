use std::collections::HashMap;
use tower_lsp_f::lsp_types::{
    CodeAction, CodeActionKind, CodeActionResponse, Position, Range, TextEdit, Uri, WorkspaceEdit,
};

use crate::document::DocumentState;

pub fn generate_extract_variable_action(
    state: &DocumentState,
    uri: &Uri,
    range: Range,
) -> Option<CodeActionResponse> {
    if range.start == range.end {
        return None;
    }

    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    let start_col = range.start.character as usize;
    let end_col = range.end.character as usize;

    let lines: Vec<&str> = state.source.lines().collect();
    if start_line >= lines.len() || end_line >= lines.len() {
        return None;
    }

    let selected_text = if start_line == end_line {
        let line = lines[start_line];
        if start_col >= line.len() || end_col > line.len() || start_col >= end_col {
            return None;
        }
        line[start_col..end_col].trim()
    } else {
        return None;
    };

    if selected_text.is_empty() || selected_text.contains(';') {
        return None;
    }

    let line_str = lines[start_line];
    let indent_len = line_str.len() - line_str.trim_start().len();
    let indent = " ".repeat(indent_len);

    let var_name = "extractedVar";
    let insert_pos = Position {
        line: range.start.line,
        character: 0,
    };

    let decl_edit = TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: format!("{indent}let {var_name} = {selected_text};\n"),
    };

    let replace_edit = TextEdit {
        range,
        new_text: var_name.to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![decl_edit, replace_edit]);

    Some(CodeActionResponse::CodeAction(CodeAction {
        title: format!("Extract '{selected_text}' into variable '{var_name}'"),
        kind: Some(CodeActionKind::RefactorExtract),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        tags: None,
        data: None,
    }))
}
