use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{AstArena, Decl, Program, StmtKind};
use varn_core::SourceRange;

use crate::document::DocumentState;

pub fn generate_organize_imports_action(
    state: &DocumentState,
    uri: &tower_lsp::lsp_types::Url,
) -> Option<CodeActionOrCommand> {
    let program = state.ast.as_ref()?;
    let ranges = import_ranges(program, &state.ast_arena);
    let imports = collect_imports(&ranges, &state.source)?;

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

    let first_line = ranges.first()?.start.line.saturating_sub(1);
    let last_line = ranges.last()?.end.line.saturating_sub(1);

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

/// The source range of each top-level import, in order.
fn import_ranges(program: &Program, arena: &AstArena) -> Vec<SourceRange> {
    program
        .body
        .iter()
        .map(|&id| arena.stmt(id))
        .filter(
            |stmt| matches!(&stmt.kind, StmtKind::Decl(d) if matches!(d.as_ref(), Decl::Import(_))),
        )
        .map(|stmt| stmt.range)
        .collect()
}

/// The text of each import, whole lines.
fn collect_imports(ranges: &[SourceRange], source: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = source.lines().collect();
    let import_lines: Vec<String> = ranges
        .iter()
        .filter_map(|r| {
            let s_line = r.start.line.saturating_sub(1) as usize;
            let e_line = r.end.line.saturating_sub(1) as usize;
            (s_line < lines.len() && e_line < lines.len())
                .then(|| lines[s_line..=e_line].join("\n"))
        })
        .collect();
    (!import_lines.is_empty()).then_some(import_lines)
}
