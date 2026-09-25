use std::sync::Arc;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range,
    SymbolKind as LspSymbolKind, Url,
};
use varn_checker::SymbolKind;
use varn_core::ast::{ClassMember, Decl, ExprId, ExprKind, StmtKind};
use varn_core::SourceRange;

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn prepare_call_hierarchy(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<Vec<CallHierarchyItem>> {
    let token = state.identifier_token_at(line, col)?;
    let target_name = &token.lexeme;

    for sym in state.symbols() {
        if sym.name() == *target_name
            && matches!(sym.kind(), SymbolKind::Function | SymbolKind::Method)
            && sym.line() != u32::MAX
        {
            let url = Url::parse(&state.uri).ok()?;
            let range = Range {
                start: Position {
                    line: sym.line(),
                    character: sym.col(),
                },
                end: Position {
                    line: sym.end_line(),
                    character: sym.end_col(),
                },
            };
            let selection_range = Range {
                start: Position {
                    line: sym.line(),
                    character: sym.col(),
                },
                end: Position {
                    line: sym.line(),
                    character: sym.col() + sym.name().len() as u32,
                },
            };

            let item = CallHierarchyItem {
                name: sym.name().to_owned(),
                kind: LspSymbolKind::FUNCTION,
                tags: None,
                detail: Some(sym.type_str()),
                uri: url,
                range,
                selection_range,
                data: Some(serde_json::Value::String(sym.global_key(true))),
            };
            return Some(vec![item]);
        }
    }

    None
}

pub fn incoming_calls(
    item: CallHierarchyItem,
    workspace: &Workspace,
) -> Option<Vec<CallHierarchyIncomingCall>> {
    let target_name = &item.name;
    let mut incoming = Vec::new();

    let entries: Vec<(String, Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &entries {
        let url = match Url::parse(file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        for (caller_fn_name, caller_range, call_range) in find_calls_to(file_state, target_name) {
            let caller_item = CallHierarchyItem {
                name: caller_fn_name,
                kind: LspSymbolKind::FUNCTION,
                tags: None,
                detail: None,
                uri: url.clone(),
                range: caller_range,
                selection_range: caller_range,
                data: None,
            };

            incoming.push(CallHierarchyIncomingCall {
                from: caller_item,
                from_ranges: vec![call_range],
            });
        }
    }

    if incoming.is_empty() {
        None
    } else {
        Some(incoming)
    }
}

pub fn outgoing_calls(
    item: CallHierarchyItem,
    workspace: &Workspace,
) -> Option<Vec<CallHierarchyOutgoingCall>> {
    let uri_str = item.uri.to_string();
    let state = workspace.get(&uri_str)?;

    let (_, target_range) = callables(&state)
        .into_iter()
        .find(|(name, _)| *name == item.name)?;
    let mut outgoing = Vec::new();

    for (callee_name, call_range) in calls_in(&state).filter(|(_, r)| encloses(&target_range, r)) {
        let call_range = to_lsp_range(&call_range);
        let callee_item = CallHierarchyItem {
            name: callee_name,
            kind: LspSymbolKind::FUNCTION,
            tags: None,
            detail: None,
            uri: item.uri.clone(),
            range: call_range,
            selection_range: call_range,
            data: None,
        };

        outgoing.push(CallHierarchyOutgoingCall {
            to: callee_item,
            from_ranges: vec![call_range],
        });
    }

    if outgoing.is_empty() {
        None
    } else {
        Some(outgoing)
    }
}

/// The functions and methods `file` declares at its top level, by name,
/// with their ranges.
fn callables(file: &DocumentState) -> Vec<(String, SourceRange)> {
    let body = file
        .ast
        .as_ref()
        .map(|p| p.body.as_slice())
        .unwrap_or_default();
    let mut out = Vec::new();
    for &id in body {
        let StmtKind::Decl(decl) = &file.ast_arena.stmt(id).kind else {
            continue;
        };
        match decl.as_ref() {
            Decl::Function(f) => out.push((file.name(f.id).to_owned(), f.range)),
            Decl::Class(c) => {
                for member in &c.body {
                    if let ClassMember::Method {
                        key,
                        body: Some(_),
                        range,
                        ..
                    } = member
                    {
                        out.push((file.name(*key).to_owned(), *range));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every call in `file` whose callee is named — `f(..)`, `x.f(..)` — with
/// that name and the call's range.
fn calls_in(file: &DocumentState) -> impl Iterator<Item = (String, SourceRange)> + '_ {
    let arena = &file.ast_arena;
    let name_of = move |id: ExprId| match &arena.expr(id).kind {
        ExprKind::Identifier { name } => Some(file.name(*name).to_owned()),
        _ => None,
    };
    file.spatial_index.exprs().filter_map(move |id| {
        let call = arena.expr(id);
        let ExprKind::Call { callee, .. } = &call.kind else {
            return None;
        };
        let name = match &arena.expr(*callee).kind {
            ExprKind::Member {
                property,
                computed: false,
                ..
            } => name_of(*property),
            _ => name_of(*callee),
        }?;
        Some((name, call.range))
    })
}

fn encloses(outer: &SourceRange, inner: &SourceRange) -> bool {
    outer.start.offset <= inner.start.offset && inner.end.offset <= outer.end.offset
}

/// The calls of `target_callee` in `file`, each with the function or method
/// it is made from.
fn find_calls_to(file: &DocumentState, target_callee: &str) -> Vec<(String, Range, Range)> {
    let callers = callables(file);
    calls_in(file)
        .filter(|(callee, _)| callee == target_callee)
        .filter_map(|(_, call)| {
            // The innermost caller: a method's range lies inside its class's.
            let (name, range) = callers
                .iter()
                .filter(|(_, r)| encloses(r, &call))
                .min_by_key(|(_, r)| r.end.offset - r.start.offset)?;
            Some((name.clone(), to_lsp_range(range), to_lsp_range(&call)))
        })
        .collect()
}

fn to_lsp_range(r: &varn_core::SourceRange) -> Range {
    Range {
        start: Position {
            line: r.start.line.saturating_sub(1),
            character: r.start.column,
        },
        end: Position {
            line: r.end.line.saturating_sub(1),
            character: r.end.column,
        },
    }
}
