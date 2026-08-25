use std::sync::Arc;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};
use varn_core::ast::{ClassDecl, ClassMember, Decl, Program, StmtKind, TypeNode};
use varn_core::TypeKind;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn build_goto_implementation(
    state: &DocumentState,
    workspace: &Workspace,
    line: u32,
    col: u32,
) -> Option<GotoDefinitionResponse> {
    let token = state.identifier_token_at(line, col)?;
    let target_name = &token.lexeme;

    // Check if target is an interface or class in current file
    let (is_interface, is_class_or_method) = {
        let is_iface = state.symbols().any(|s| {
            s.name() == *target_name && s.kind() == varn_checker::SymbolKind::Interface
        });
        let is_cls = state.symbols().any(|s| {
            s.name() == *target_name
                && matches!(
                    s.kind(),
                    varn_checker::SymbolKind::Class | varn_checker::SymbolKind::Method
                )
        });
        (is_iface, is_cls)
    };

    let mut locations = Vec::new();

    let entries: Vec<(String, Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &entries {
        let url = match Url::parse(file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if let Some(program) = &file_state.ast {
            if is_interface {
                find_interface_implementations(program, target_name, &url, &mut locations);
            } else if is_class_or_method {
                find_class_subtypes(program, target_name, &url, &mut locations);
            }
        }
    }

    if locations.is_empty() {
        None
    } else if locations.len() == 1 {
        Some(GotoDefinitionResponse::Scalar(
            locations.into_iter().next().unwrap(),
        ))
    } else {
        Some(GotoDefinitionResponse::Array(locations))
    }
}

fn find_interface_implementations(
    program: &Program,
    iface_name: &str,
    url: &Url,
    locations: &mut Vec<Location>,
) {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Class(c) = decl.as_ref() {
                let implements_iface = c
                    .implements
                    .iter()
                    .any(|t| type_node_name(t) == iface_name);

                if implements_iface {
                    locations.push(class_location(c, url));
                }
            }
        }
    }
}

fn find_class_subtypes(
    program: &Program,
    class_or_method_name: &str,
    url: &Url,
    locations: &mut Vec<Location>,
) {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Class(c) = decl.as_ref() {
                // Check if extends class
                if let Some(super_expr) = &c.super_class {
                    if let varn_core::ast::ExprKind::Identifier { name } = &super_expr.kind {
                        if name.as_ref() == class_or_method_name {
                            locations.push(class_location(c, url));
                        }
                    }
                }

                // Check if class implements method with this name
                for member in &c.body {
                    if let ClassMember::Method { key, range, .. } = member {
                        if key.as_ref() == class_or_method_name {
                            locations.push(Location::new(
                                url.clone(),
                                Range {
                                    start: Position {
                                        line: range.start.line.saturating_sub(1),
                                        character: range.start.column,
                                    },
                                    end: Position {
                                        line: range.end.line.saturating_sub(1),
                                        character: range.end.column,
                                    },
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn type_node_name(node: &TypeNode) -> String {
    match &node.kind {
        TypeKind::Named(name, _) => name.clone(),
        TypeKind::Generic(name, _, _) => name.clone(),
        TypeKind::Intrinsic(tag) => format!("{:?}", tag).to_lowercase(),
        _ => "any".to_string(),
    }
}

fn class_location(c: &ClassDecl, url: &Url) -> Location {
    let s_line = c.range.start.line.saturating_sub(1);
    let s_col = c.range.start.column;
    let name_len = c.id.as_deref().map(|n| n.len()).unwrap_or(5) as u32;

    Location::new(
        url.clone(),
        Range {
            start: Position {
                line: s_line,
                character: s_col,
            },
            end: Position {
                line: s_line,
                character: s_col + name_len,
            },
        },
    )
}
