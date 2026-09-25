use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{ClassDecl, ClassMember, Decl, InterfaceDecl, InterfaceMember, StmtKind};

use crate::document::DocumentState;
use crate::index::ProjectIndex;

pub fn generate_interface_impl_action(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    uri: &tower_lsp::lsp_types::Url,
    cursor_line: u32,
    _cursor_col: u32,
) -> Option<CodeActionOrCommand> {
    let program = state.ast.as_ref()?;
    let class_decl = find_class_at_line(state, program, cursor_line)?;

    if class_decl.implements.is_empty() {
        return None;
    }

    let existing_methods: Vec<String> = class_decl
        .body
        .iter()
        .filter_map(|m| match m {
            ClassMember::Method { key, .. } => Some(state.name(*key).to_owned()),
            _ => None,
        })
        .collect();

    for iface_type in &class_decl.implements {
        let Some(iface_name) = state.type_node_decl_name(iface_type) else {
            continue;
        };

        let iface_decl = find_interface(state, program, index, iface_name);
        if let Some(iface) = iface_decl {
            let mut missing_methods = Vec::new();
            for member in &iface.body {
                if let InterfaceMember::Method {
                    key,
                    params,
                    return_type,
                    is_async,
                    ..
                } = member
                {
                    let key = state.name(*key);
                    if !existing_methods.iter().any(|m| m == key) {
                        missing_methods.push((
                            key.to_owned(),
                            params.clone(),
                            return_type.clone(),
                            *is_async,
                        ));
                    }
                }
            }

            if missing_methods.is_empty() {
                continue;
            }

            let class_range = &class_decl.range;
            let insert_line = class_range.end.line.saturating_sub(1);
            let insert_col = class_range.end.column.saturating_sub(1);

            let mut stubs = String::new();
            let indent = "    ";
            for (name, params, ret, is_async) in &missing_methods {
                let async_prefix = if *is_async { "async " } else { "" };
                let params_str: Vec<String> = params
                    .iter()
                    .map(|p| {
                        let ty_str = p
                            .type_ann
                            .as_ref()
                            .map(|t| format!(": {}", state.source_text(t.range)))
                            .unwrap_or_default();
                        match &p.pattern {
                            varn_core::ast::Pattern::Identifier { name, .. } => {
                                format!("{}{}", state.name(*name), ty_str)
                            }
                            _ => format!("arg{}", ty_str),
                        }
                    })
                    .collect();

                let ret_str = ret
                    .as_ref()
                    .map(|t| format!(": {}", state.source_text(t.range)))
                    .unwrap_or_default();

                stubs.push_str(&format!(
                    "\n{indent}{async_prefix}{name}({}){} {{\n{indent}    throw new Error(\"Method '{name}' not implemented\");\n{indent}}}\n",
                    params_str.join(", "),
                    ret_str
                ));
            }

            let insert_pos = Position {
                line: insert_line,
                character: insert_col,
            };

            let mut changes = HashMap::new();
            changes.insert(
                uri.clone(),
                vec![TextEdit {
                    range: Range {
                        start: insert_pos,
                        end: insert_pos,
                    },
                    new_text: stubs,
                }],
            );

            return Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("💡 Implement missing members of interface '{}'", iface_name),
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
            }));
        }
    }

    None
}

fn find_class_at_line<'a>(
    state: &'a DocumentState,
    program: &varn_core::ast::Program,
    line: u32,
) -> Option<&'a ClassDecl> {
    program
        .body
        .iter()
        .find_map(|&id| match &state.ast_arena.stmt(id).kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Class(c)
                    if (c.range.start.line.saturating_sub(1)
                        ..=c.range.end.line.saturating_sub(1))
                        .contains(&line) =>
                {
                    Some(c)
                }
                _ => None,
            },
            _ => None,
        })
}

/// The interface `name` this document declares at its top level.
fn find_interface(
    state: &DocumentState,
    program: &varn_core::ast::Program,
    _index: Option<&ProjectIndex>,
    name: &str,
) -> Option<InterfaceDecl> {
    program
        .body
        .iter()
        .find_map(|&id| match &state.ast_arena.stmt(id).kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Interface(i) if state.name(i.id) == name => Some(i.clone()),
                _ => None,
            },
            _ => None,
        })
}
