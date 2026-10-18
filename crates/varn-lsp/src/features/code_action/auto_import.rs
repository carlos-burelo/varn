use tower_lsp_f::lsp_types::{
    CodeAction, CodeActionKind, CodeActionResponse, Diagnostic, Position, Range, TextEdit,
};

use crate::document::DocumentState;
use crate::index::ProjectIndex;
use varn_sem::resolver::ImportResolver;

pub fn generate_auto_imports_action(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    resolver: &varn_resolver::DiskResolver,
    uri: &tower_lsp_f::lsp_types::Uri,
    diag: &Diagnostic,
) -> Vec<CodeActionResponse> {
    let mut actions = Vec::new();

    let missing_name = extract_undefined_symbol_name(diag_message_text(&diag.message));
    let sym_name = match missing_name {
        Some(n) => n.trim(),
        None => return actions,
    };

    if sym_name.is_empty() {
        return actions;
    }

    for module in varn_modules::std_module_ids() {
        if resolver.stdlib_exports(module).contains_key(sym_name)
            && let Some(action) = create_import_action(uri, diag, sym_name, module)
        {
            actions.push(action);
        }
    }

    if let Some(idx) = index {
        let defs = idx.definitions_of(sym_name);
        for entry in defs {
            if entry.uri.as_ref() != uri.as_str() {
                let rel_path = compute_relative_import_path(uri.as_str(), &entry.uri);
                if let Some(action) = create_import_action(uri, diag, sym_name, &rel_path) {
                    actions.push(action);
                }
            }
        }
    }

    if actions.is_empty() {
        for s in state.symbols() {
            if s.is_from_stdlib()
                && s.name() == sym_name
                && let Some(origin) = &s.origin()
                && let Some(action) = create_import_action(uri, diag, sym_name, origin)
            {
                actions.push(action);
            }
        }
    }

    actions
}

fn diag_message_text(message: &tower_lsp_f::lsp_types::Message) -> &str {
    match message {
        tower_lsp_f::lsp_types::Message::String(s) => s,
        tower_lsp_f::lsp_types::Message::MarkupContent(m) => &m.value,
    }
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
    uri: &tower_lsp_f::lsp_types::Uri,
    diag: &Diagnostic,
    sym_name: &str,
    module: &str,
) -> Option<CodeActionResponse> {
    let edit = TextEdit {
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
    };

    Some(CodeActionResponse::CodeAction(CodeAction {
        title: format!("💡 Import {{ {sym_name} }} from \"{module}\""),
        kind: Some(CodeActionKind::QuickFix),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(crate::features::workspace_edit::doc_edits(vec![(
            uri.clone(),
            vec![edit],
        )])),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        tags: None,
        data: None,
    }))
}

fn compute_relative_import_path(from_uri: &str, to_uri: &str) -> String {
    let from_path = crate::document::uri_to_path(from_uri);
    let to_path = crate::document::uri_to_path(to_uri);

    let from_p = std::path::Path::new(&from_path);
    let to_p = std::path::Path::new(&to_path);

    if let (Some(from_dir), Some(to_file)) = (from_p.parent(), to_p.file_name())
        && from_dir == to_p.parent().unwrap_or(from_dir)
    {
        return format!("./{}", to_file.to_string_lossy());
    }

    to_uri.to_string()
}
