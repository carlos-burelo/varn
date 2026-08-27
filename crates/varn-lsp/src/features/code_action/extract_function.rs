use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Url, WorkspaceEdit,
};

use crate::document::DocumentState;

pub fn generate_extract_function_action(
    state: &DocumentState,
    uri: &Url,
    range: Range,
) -> Option<CodeActionOrCommand> {
    if range.start == range.end {
        return None;
    }

    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    let lines: Vec<&str> = state.source.lines().collect();

    if start_line >= lines.len() || end_line >= lines.len() {
        return None;
    }

    let mut selected_body = String::new();
    if start_line == end_line {
        let line = lines[start_line];
        let start_col = (range.start.character as usize).min(line.len());
        let end_col = (range.end.character as usize).min(line.len());
        if start_col >= end_col {
            return None;
        }
        selected_body.push_str(&line[start_col..end_col]);
    } else {
        for (idx, line) in lines.iter().enumerate().take(end_line + 1).skip(start_line) {
            if idx == start_line {
                let start_col = (range.start.character as usize).min(line.len());
                selected_body.push_str(&line[start_col..]);
                selected_body.push('\n');
            } else if idx == end_line {
                let end_col = (range.end.character as usize).min(line.len());
                selected_body.push_str(&line[..end_col]);
            } else {
                selected_body.push_str(line);
                selected_body.push('\n');
            }
        }
    }

    let trimmed = selected_body.trim();
    if trimmed.is_empty() {
        return None;
    }

    let fn_name = "newFunction";
    let indent_len = lines[start_line].len() - lines[start_line].trim_start().len();
    let indent = " ".repeat(indent_len);

    let fn_def = format!("\n{indent}fn {fn_name}() {{\n{indent}    {trimmed}\n{indent}}}\n");

    let insert_pos = Position {
        line: range.start.line,
        character: 0,
    };

    let def_edit = TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: fn_def,
    };

    let call_edit = TextEdit {
        range,
        new_text: format!("{fn_name}()"),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![def_edit, call_edit]);

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Extract into function '{fn_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }))
}
