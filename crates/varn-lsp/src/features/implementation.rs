use std::sync::Arc;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};
use varn_core::ast::{ClassDecl, ClassMember, Decl, StmtKind};

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
        let is_iface = state
            .symbols()
            .any(|s| s.name() == *target_name && s.kind() == varn_checker::SymbolKind::Interface);
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

        if is_interface {
            find_interface_implementations(file_state, target_name, &url, &mut locations);
        } else if is_class_or_method {
            find_class_subtypes(file_state, target_name, &url, &mut locations);
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

/// The classes declared at `file`'s top level.
fn top_level_classes(file: &DocumentState) -> impl Iterator<Item = &ClassDecl> {
    let body = file
        .ast
        .as_ref()
        .map(|p| p.body.as_slice())
        .unwrap_or_default();
    body.iter()
        .filter_map(|&id| match &file.ast_arena.stmt(id).kind {
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Class(c) => Some(c),
                _ => None,
            },
            _ => None,
        })
}

fn find_interface_implementations(
    file: &DocumentState,
    iface_name: &str,
    url: &Url,
    locations: &mut Vec<Location>,
) {
    for c in top_level_classes(file) {
        if c.implements
            .iter()
            .any(|t| file.type_node_decl_name(t) == Some(iface_name))
        {
            locations.push(class_location(file, c, url));
        }
    }
}

fn find_class_subtypes(
    file: &DocumentState,
    class_or_method_name: &str,
    url: &Url,
    locations: &mut Vec<Location>,
) {
    for c in top_level_classes(file) {
        // A class extending it.
        if let Some(super_expr) = c.super_class {
            if let varn_core::ast::ExprKind::Identifier { name } =
                &file.ast_arena.expr(super_expr).kind
            {
                if file.name(*name) == class_or_method_name {
                    locations.push(class_location(file, c, url));
                }
            }
        }

        // A class declaring a method of that name.
        for member in &c.body {
            if let ClassMember::Method { key, range, .. } = member {
                if file.name(*key) == class_or_method_name {
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

fn class_location(file: &DocumentState, c: &ClassDecl, url: &Url) -> Location {
    let s_line = c.range.start.line.saturating_sub(1);
    let s_col = c.range.start.column;
    let name_len = c.id.map_or(5, |n| file.name(n).len()) as u32;

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
