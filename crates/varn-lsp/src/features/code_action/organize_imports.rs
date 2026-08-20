use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{Decl, Program, StmtKind};

use crate::document::DocumentState;

pub fn generate_organize_imports_action(
    state: &DocumentState,
    uri: &tower_lsp::lsp_types::Url,
) -> Option<CodeActionOrCommand> {
    let program = state.ast.as_ref()?;
    let imports = collect_imports(program, &state.source)?;

    if imports.len() < 2 {
        return None;
    }

    let mut std_imports: Vec<String> = Vec::new();
    let mut other_imports: Vec<String> = Vec::new();

    for imp in &imports {
        if imp.contains("\"std:") || imp.contains("'std:") {
            std_imports.push(imp.clone());
        } else {
            other_imports.push(imp.clone());
        }
    }

    std_imports.sort();
    std_imports.dedup();
    other_imports.sort();
    other_imports.dedup();

    let mut organized = String::new();
    for imp in &std_imports {
        organized.push_str(imp);
        organized.push('\n');
    }
    if !std_imports.is_empty() && !other_imports.is_empty() {
        organized.push('\n');
    }
    for imp in &other_imports {
        organized.push_str(imp);
        organized.push('\n');
    }

    let first_line = imports_first_line(program)?;
    let last_line = imports_last_line(program)?;

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: first_line,
                    character: 0,
                },
                end: Position {
                    line: last_line + 1,
                    character: 0,
                },
            },
            new_text: organized,
        }],
    );

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Organize Imports".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
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

fn collect_imports(program: &Program, source: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut import_lines = Vec::new();

    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Import(_) = decl.as_ref() {
                let s_line = stmt.range().start.line.saturating_sub(1) as usize;
                let e_line = stmt.range().end.line.saturating_sub(1) as usize;
                if s_line < lines.len() && e_line < lines.len() {
                    let text = lines[s_line..=e_line].join("\n");
                    import_lines.push(text);
                }
            }
        }
    }

    if import_lines.is_empty() {
        None
    } else {
        Some(import_lines)
    }
}

fn imports_first_line(program: &Program) -> Option<u32> {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Import(_) = decl.as_ref() {
                return Some(stmt.range().start.line.saturating_sub(1));
            }
        }
    }
    None
}

fn imports_last_line(program: &Program) -> Option<u32> {
    let mut last = None;
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Import(_) = decl.as_ref() {
                last = Some(stmt.range().end.line.saturating_sub(1));
            }
        }
    }
    last
}
