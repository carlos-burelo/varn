use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, WorkspaceEdit,
};
use varn_core::ast::{
    ClassDecl, ClassMember, Decl, InterfaceDecl, InterfaceMember, Program, StmtKind, TypeNode,
};
use varn_core::TypeKind;

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
    let class_decl = find_class_at_line(program, cursor_line)?;

    if class_decl.implements.is_empty() {
        return None;
    }

    let existing_methods: Vec<String> = class_decl
        .body
        .iter()
        .filter_map(|m| match m {
            ClassMember::Method { key, .. } => Some(key.to_string()),
            _ => None,
        })
        .collect();

    for iface_type in &class_decl.implements {
        let iface_name = type_node_name(iface_type);
        if iface_name.is_empty() {
            continue;
        }

        let iface_decl = find_interface(program, index, &iface_name);
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
                    if !existing_methods.contains(&key.to_string()) {
                        missing_methods.push((
                            key.to_string(),
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
                            .map(|t| format!(": {}", type_node_name(t)))
                            .unwrap_or_default();
                        match &p.pattern {
                            varn_core::ast::Pattern::Identifier { name, .. } => {
                                format!("{}{}", name, ty_str)
                            }
                            _ => format!("arg{}", ty_str),
                        }
                    })
                    .collect();

                let ret_str = ret
                    .as_ref()
                    .map(|t| format!(": {}", type_node_name(t)))
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

pub fn type_node_name(node: &TypeNode) -> String {
    match &node.kind {
        TypeKind::Named(name, _) => name.clone(),
        TypeKind::Generic(name, _, _) => name.clone(),
        TypeKind::Intrinsic(tag) => format!("{:?}", tag).to_lowercase(),
        _ => "any".to_string(),
    }
}

fn find_class_at_line(program: &Program, line: u32) -> Option<&ClassDecl> {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Class(c) = decl.as_ref() {
                let s_line = c.range.start.line.saturating_sub(1);
                let e_line = c.range.end.line.saturating_sub(1);
                if line >= s_line && line <= e_line {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn find_interface(
    program: &Program,
    _index: Option<&ProjectIndex>,
    name: &str,
) -> Option<InterfaceDecl> {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Interface(i) = decl.as_ref() {
                if i.id.as_ref() == name {
                    return Some(i.clone());
                }
            }
        }
    }
    None
}
