use std::sync::Arc;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range,
    SymbolKind as LspSymbolKind, Url,
};
use varn_checker::SymbolKind;
use varn_core::ast::{Arg, Decl, Expr, ExprKind, Program, Stmt, StmtKind};

use crate::document::DocumentState;
use crate::workspace::Workspace;

pub fn prepare_call_hierarchy(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<Vec<CallHierarchyItem>> {
    let token = state.identifier_token_at(line, col)?;
    let target_name = &token.lexeme;

    for sym in &state.symbols {
        if sym.name == *target_name
            && matches!(
                sym.kind,
                SymbolKind::Function | SymbolKind::Method
            )
            && sym.line != u32::MAX
        {
            let url = Url::parse(&state.uri).ok()?;
            let range = Range {
                start: Position {
                    line: sym.line,
                    character: sym.col,
                },
                end: Position {
                    line: sym.end_line,
                    character: sym.end_col,
                },
            };
            let selection_range = Range {
                start: Position {
                    line: sym.line,
                    character: sym.col,
                },
                end: Position {
                    line: sym.line,
                    character: sym.col + sym.name.len() as u32,
                },
            };

            let item = CallHierarchyItem {
                name: sym.name.clone(),
                kind: LspSymbolKind::FUNCTION,
                tags: None,
                detail: Some(sym.type_str.clone()),
                uri: url,
                range,
                selection_range,
                data: Some(serde_json::Value::String(sym.global_key.clone())),
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

        if let Some(program) = &file_state.ast {
            let calls = find_calls_in_program(program, target_name);
            for (caller_fn_name, caller_range, call_range) in calls {
                let caller_item = CallHierarchyItem {
                    name: caller_fn_name.clone(),
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
    let program = state.ast.as_ref()?;

    let target_fn = find_function_in_program(program, &item.name)?;
    let mut outgoing = Vec::new();

    let calls = collect_callees_in_stmt(&target_fn.body);
    for (callee_name, call_range) in calls {
        let callee_item = CallHierarchyItem {
            name: callee_name.clone(),
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

fn find_calls_in_program(
    program: &Program,
    target_callee: &str,
) -> Vec<(String, Range, Range)> {
    let mut results = Vec::new();
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            match decl.as_ref() {
                Decl::Function(f) => {
                    let f_range = to_lsp_range(&f.range);
                    let calls = collect_callees_in_stmt(&f.body);
                    for (callee, call_range) in calls {
                        if callee == target_callee {
                            results.push((f.id.to_string(), f_range, call_range));
                        }
                    }
                }
                Decl::Class(c) => {
                    for member in &c.body {
                        if let varn_core::ast::ClassMember::Method {
                            key,
                            body: Some(b),
                            range,
                            ..
                        } = member
                        {
                            let m_range = to_lsp_range(range);
                            let calls = collect_callees_in_stmt(b);
                            for (callee, call_range) in calls {
                                if callee == target_callee {
                                    results.push((key.to_string(), m_range, call_range));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    results
}

fn find_function_in_program<'a>(
    program: &'a Program,
    name: &str,
) -> Option<&'a varn_core::ast::FunctionDecl> {
    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Function(f) = decl.as_ref() {
                if f.id.as_ref() == name {
                    return Some(f);
                }
            }
        }
    }
    None
}

fn collect_callees_in_stmt(stmt: &Stmt) -> Vec<(String, Range)> {
    let mut results = Vec::new();
    match &stmt.kind {
        StmtKind::Expr { expression } => collect_callees_in_expr(expression, &mut results),
        StmtKind::Block { stmts } => {
            for s in stmts {
                results.extend(collect_callees_in_stmt(s));
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_callees_in_expr(test, &mut results);
            results.extend(collect_callees_in_stmt(consequent));
            if let Some(alt) = alternate {
                results.extend(collect_callees_in_stmt(alt));
            }
        }
        StmtKind::Return { argument: Some(e) } => collect_callees_in_expr(e, &mut results),
        _ => {}
    }
    results
}

fn collect_callees_in_expr(expr: &Expr, results: &mut Vec<(String, Range)>) {
    match &expr.kind {
        ExprKind::Call { callee, args, .. } => {
            if let ExprKind::Identifier { name } = &callee.kind {
                results.push((name.to_string(), to_lsp_range(&expr.range)));
            } else if let ExprKind::Member { property, .. } = &callee.kind {
                if let ExprKind::Identifier { name } = &property.kind {
                    results.push((name.to_string(), to_lsp_range(&expr.range)));
                }
            }
            for arg in args {
                let arg_expr = match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => e,
                };
                collect_callees_in_expr(arg_expr, results);
            }
        }
        ExprKind::Binary { left, right, .. } => {
            collect_callees_in_expr(left, results);
            collect_callees_in_expr(right, results);
        }
        ExprKind::Unary { operand, .. } => collect_callees_in_expr(operand, results),
        _ => {}
    }
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
