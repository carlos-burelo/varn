use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, Position, Range, TextEdit,
    WorkspaceEdit,
};

use crate::document::DocumentState;
use crate::index::ProjectIndex;

const STDLIB_COMMON_EXPORTS: &[(&str, &str)] = &[
    ("File", "std:fs"),
    ("Path", "std:path"),
    ("readTextFile", "std:fs"),
    ("writeTextFile", "std:fs"),
    ("JSON", "std:json"),
    ("HttpClient", "std:http"),
    ("HttpServer", "std:http"),
    ("Isolate", "std:isolate"),
    ("Channel", "std:channel"),
    ("Timer", "std:time"),
    ("sleep", "std:time"),
    ("Duration", "std:time"),
    ("Instant", "std:time"),
    ("Process", "std:process"),
    ("env", "std:env"),
    ("Regex", "std:regex"),
    ("Crypto", "std:crypto"),
    ("HashMap", "std:collections"),
    ("HashSet", "std:collections"),
    ("sin", "std:math"),
    ("cos", "std:math"),
    ("tan", "std:math"),
    ("sqrt", "std:math"),
    ("PI", "std:math"),
    ("E", "std:math"),
    ("assert", "std:test"),
    ("assertEqual", "std:test"),
];

pub fn generate_auto_imports_action(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    uri: &tower_lsp::lsp_types::Url,
    diag: &Diagnostic,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    let missing_name = extract_undefined_symbol_name(&diag.message);
    let sym_name = match missing_name {
        Some(n) => n.trim(),
        None => return actions,
    };

    if sym_name.is_empty() {
        return actions;
    }

    // Check stdlib common exports
    for &(exp, module) in STDLIB_COMMON_EXPORTS {
        if exp == sym_name {
            if let Some(action) = create_import_action(uri, diag, sym_name, module) {
                actions.push(action);
            }
        }
    }

    // Check project index for workspace definitions
    if let Some(idx) = index {
        let defs = idx.definitions_of(sym_name);
        for (file_uri, _) in defs {
            if file_uri != uri.as_str() {
                let rel_path = compute_relative_import_path(uri.as_str(), &file_uri);
                if let Some(action) = create_import_action(uri, diag, sym_name, &rel_path) {
                    actions.push(action);
                }
            }
        }
    }

    // Fallback: check symbols in DocumentState stdlib symbols
    if actions.is_empty() {
        for s in state.symbols() {
            if s.is_from_stdlib() && s.name() == sym_name {
                if let Some(origin) = &s.origin() {
                    if let Some(action) = create_import_action(uri, diag, sym_name, origin) {
                        actions.push(action);
                    }
                }
            }
        }
    }

    actions
}

fn extract_undefined_symbol_name(message: &str) -> Option<&str> {
    if let Some(rest) = message.split("undefined variable:").nth(1) {
        return Some(rest);
    }
    if let Some(rest) = message.split("cannot find name '").nth(1) {
        return rest.split('\'').next();
    }
    if let Some(rest) = message.split("undefined symbol '").nth(1) {
        return rest.split('\'').next();
    }
    None
}

fn create_import_action(
    uri: &tower_lsp::lsp_types::Url,
    diag: &Diagnostic,
    sym_name: &str,
    module: &str,
) -> Option<CodeActionOrCommand> {
    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: format!("import {{ {sym_name} }} from \"{module}\"\n"),
        }],
    );

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("💡 Import {{ {sym_name} }} from \"{module}\""),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    }))
}

fn compute_relative_import_path(from_uri: &str, to_uri: &str) -> String {
    let from_path = crate::document::uri_to_path(from_uri);
    let to_path = crate::document::uri_to_path(to_uri);

    let from_p = std::path::Path::new(&from_path);
    let to_p = std::path::Path::new(&to_path);

    if let (Some(from_dir), Some(to_file)) = (from_p.parent(), to_p.file_name()) {
        if from_dir == to_p.parent().unwrap_or(from_dir) {
            return format!("./{}", to_file.to_string_lossy());
        }
    }

    to_uri.to_string()
}
