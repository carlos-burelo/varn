use std::sync::Arc;
use tower_lsp_f::lsp_types::{Definition, Location, Position, Range, Uri};
use varn_core::ast::{ClassDecl, ClassMember};

use crate::document::classes::top_level_classes;
use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn build_goto_implementation(
    state: &DocumentState,
    workspace: &Workspace,
    line: u32,
    col: u32,
) -> Option<Definition> {
    let token = state.identifier_token_at(line, col)?;
    let target_name = state.lexeme(token);

    let (is_interface, is_class_or_method) = {
        let is_iface = state.symbols().any(|s| {
            s.name() == target_name && s.kind() == varn_sem::symbol::SymbolKind::Interface
        });
        let is_cls = state.symbols().any(|s| {
            s.name() == target_name
                && matches!(
                    s.kind(),
                    varn_sem::symbol::SymbolKind::Class | varn_sem::symbol::SymbolKind::Method
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
        let uri = match Uri::parse(file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        if is_interface {
            find_interface_implementations(file_state, target_name, &uri, &mut locations);
        } else if is_class_or_method {
            find_class_subtypes(file_state, target_name, &uri, &mut locations);
        }
    }

    if locations.is_empty() {
        None
    } else if locations.len() == 1 {
        Some(Definition::Location(locations.swap_remove(0)))
    } else {
        Some(Definition::LocationList(locations))
    }
}

fn find_interface_implementations(
    file: &DocumentState,
    iface_name: &str,
    uri: &Uri,
    locations: &mut Vec<Location>,
) {
    for c in top_level_classes(file) {
        if c.implements
            .iter()
            .any(|t| file.type_node_decl_name(t) == Some(iface_name))
        {
            locations.push(class_location(file, c, uri));
        }
    }
}

fn find_class_subtypes(
    file: &DocumentState,
    class_or_method_name: &str,
    uri: &Uri,
    locations: &mut Vec<Location>,
) {
    for c in top_level_classes(file) {
        if let Some(super_expr) = c.super_class {
            if let varn_core::ast::ExprKind::Identifier { name } =
                &file.ast_arena.expr(super_expr).kind
            {
                if file.name(*name) == class_or_method_name {
                    locations.push(class_location(file, c, uri));
                }
            }
        }

        for member in &c.body {
            if let ClassMember::Method { key, range, .. } = member {
                if file.name(*key) == class_or_method_name {
                    locations.push(Location::new(
                        uri.clone(),
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

fn class_location(file: &DocumentState, c: &ClassDecl, uri: &Uri) -> Location {
    let s_line = c.range.start.line.saturating_sub(1);
    let s_col = c.range.start.column;
    let name_len = c.id.map_or(5, |n| file.name(n).len()) as u32;

    Location::new(
        uri.clone(),
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
