//! "Fill missing match arms": the arms the checker found a `match` lacks
//! (`CheckResult::match_gaps`), added after its last arm.

use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{ExprId, ExprKind};

use crate::document::DocumentState;

pub fn generate_match_arms_action(
    state: &DocumentState,
    uri: &tower_lsp::lsp_types::Url,
    cursor_line: u32,
    _cursor_col: u32,
) -> Option<CodeActionOrCommand> {
    let id = match_at_line(state, cursor_line)?;
    let gap = state.db.match_gaps.get(&id.index())?;
    if gap.missing.is_empty() {
        return None;
    }

    let node = state.ast_arena.expr(id);
    let ExprKind::Match { cases, .. } = &node.kind else {
        return None;
    };
    // After the last arm, or inside the braces of a `match` with none.
    let end = cases.last().map_or(node.range.start, |c| c.range.end);
    let insert_pos = Position {
        line: end.line.saturating_sub(1),
        character: end.column,
    };

    // The existing arms' indentation, or one level past the `match` line's.
    let indent_cols = cases.first().map_or_else(
        || {
            let line = state
                .source
                .lines()
                .nth(end.line.saturating_sub(1) as usize);
            line.map_or(0, |l| l.len() - l.trim_start().len()) + 4
        },
        |c| c.range.start.column as usize,
    );
    let indent = " ".repeat(indent_cols);
    let new_cases: String = gap
        .missing
        .iter()
        .map(|pattern| format!("\n{indent}{pattern} => {{\n{indent}    // TODO\n{indent}}}"))
        .collect();

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: new_cases,
        }],
    );

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("💡 Fill missing match arms ({})", gap.missing.join(", ")),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
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

/// The innermost `match` spanning `line` (0-based).
fn match_at_line(state: &DocumentState, line: u32) -> Option<ExprId> {
    let line = line + 1;
    state
        .spatial_index
        .exprs()
        .filter(|&id| {
            let node = state.ast_arena.expr(id);
            matches!(node.kind, ExprKind::Match { .. })
                && node.range.start.line <= line
                && line <= node.range.end.line
        })
        .min_by_key(|&id| {
            let r = state.ast_arena.expr(id).range;
            r.end.offset - r.start.offset
        })
}
