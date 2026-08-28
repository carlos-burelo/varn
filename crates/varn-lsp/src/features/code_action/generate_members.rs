use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Url, WorkspaceEdit,
};
use varn_core::ast::{ClassMember, Decl, Program, Stmt, StmtKind};

use crate::document::DocumentState;

pub fn generate_class_member_actions(
    state: &DocumentState,
    uri: &Url,
    cursor_line: u32,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let program = match &state.ast {
        Some(p) => p,
        None => return actions,
    };

    let target_class = find_class_at_line(program, cursor_line);
    let class = match target_class {
        Some(c) => c,
        None => return actions,
    };

    let class_name = class.id.as_deref().unwrap_or("Anonymous");
    let mut fields = Vec::new();
    let mut has_constructor = false;
    let mut methods = std::collections::HashSet::new();

    for member in &class.body {
        match member {
            ClassMember::Property { key, type_ann, .. } => {
                let ty_str = type_ann.as_ref().map(|t| format!("{t:?}")).unwrap_or_else(|| "dynamic".to_string());
                fields.push((key.to_string(), ty_str));
            }
            ClassMember::Constructor { .. } => {
                has_constructor = true;
            }
            ClassMember::Method { key, .. } => {
                methods.insert(key.to_string());
            }
            _ => {}
        }
    }

    if fields.is_empty() {
        return actions;
    }

    let insert_line = class.range.end.line.saturating_sub(1);
    let insert_pos = Position {
        line: insert_line,
        character: 0,
    };

    // 1. Generate Constructor Action
    if !has_constructor {
        let params = fields
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect::<Vec<_>>()
            .join(", ");
        let assignments = fields
            .iter()
            .map(|(n, _)| format!("        this.{n} = {n};\n"))
            .collect::<String>();

        let ctor_code = format!(
            "    constructor({params}) {{\n{assignments}    }}\n\n"
        );

        let edit = TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: ctor_code,
        };

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![edit]);

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Generate constructor for class '{class_name}'"),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
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
        }));
    }

    // 2. Generate Getters/Setters Action
    let ungenerated_fields: Vec<_> = fields
        .iter()
        .filter(|(n, _)| !methods.contains(&format!("get_{n}")) && !methods.contains(n.as_str()))
        .collect();

    if !ungenerated_fields.is_empty() {
        let mut accessors = String::new();
        for (field_name, field_ty) in &ungenerated_fields {
            accessors.push_str(&format!(
                "    get {field_name}(): {field_ty} {{\n        return this.{field_name};\n    }}\n\n"
            ));
            accessors.push_str(&format!(
                "    set {field_name}(value: {field_ty}) {{\n        this.{field_name} = value;\n    }}\n\n"
            ));
        }

        let edit = TextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: accessors,
        };

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![edit]);

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Generate getters/setters for class '{class_name}'"),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
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
        }));
    }

    actions
}

fn find_class_at_line(program: &Program, line: u32) -> Option<&varn_core::ast::ClassDecl> {
    for stmt in &program.body {
        if let Some(c) = find_class_in_stmt(stmt, line) {
            return Some(c);
        }
    }
    None
}

fn find_class_in_stmt(stmt: &Stmt, line: u32) -> Option<&varn_core::ast::ClassDecl> {
    let s_line = stmt.range.start.line.saturating_sub(1);
    let e_line = stmt.range.end.line;
    if line < s_line || line > e_line {
        return None;
    }

    match &stmt.kind {
        StmtKind::Decl(d) => match d.as_ref() {
            Decl::Class(c) => Some(c),
            _ => None,
        },
        StmtKind::Block { stmts } => {
            for s in stmts {
                if let Some(c) = find_class_in_stmt(s, line) {
                    return Some(c);
                }
            }
            None
        }
        _ => None,
    }
}
