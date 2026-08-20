use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use varn_core::ast::{Arg, Expr, ExprKind, Stmt, StmtKind};
use varn_core::TypeKind;

use crate::document::DocumentState;

pub fn build_parameter_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    let program = match &state.ast {
        Some(p) => p,
        None => return hints,
    };

    for stmt in &program.body {
        collect_param_hints_in_stmt(state, stmt, &mut hints);
    }

    hints
}

fn collect_param_hints_in_stmt(state: &DocumentState, stmt: &Stmt, hints: &mut Vec<InlayHint>) {
    match &stmt.kind {
        StmtKind::Expr { expression } => collect_param_hints_in_expr(state, expression, hints),
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_param_hints_in_stmt(state, s, hints);
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_param_hints_in_expr(state, test, hints);
            collect_param_hints_in_stmt(state, consequent, hints);
            if let Some(alt) = alternate {
                collect_param_hints_in_stmt(state, alt, hints);
            }
        }
        StmtKind::Decl(decl) => match decl.as_ref() {
            varn_core::ast::Decl::Function(f) => {
                collect_param_hints_in_stmt(state, &f.body, hints);
            }
            varn_core::ast::Decl::Class(c) => {
                for member in &c.body {
                    if let varn_core::ast::ClassMember::Method { body: Some(b), .. } = member {
                        collect_param_hints_in_stmt(state, b, hints);
                    }
                }
            }
            _ => {}
        },
        StmtKind::Return { argument: Some(e) } => collect_param_hints_in_expr(state, e, hints),
        _ => {}
    }
}

fn collect_param_hints_in_expr(state: &DocumentState, expr: &Expr, hints: &mut Vec<InlayHint>) {
    if let ExprKind::Call { callee, args, .. } = &expr.kind {
        if !args.is_empty() {
            let param_names = resolve_callee_params(state, callee);
            for (idx, arg) in args.iter().enumerate() {
                let arg_expr = match arg {
                    Arg::Positional(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => e,
                };

                if let Some(param_name) = param_names.get(idx) {
                    if !param_name.is_empty() && !param_name.starts_with('_') {
                        let arg_start = &arg_expr.range.start;
                        let line = arg_start.line.saturating_sub(1);
                        let col = arg_start.column;

                        // Only add if argument isn't already the same identifier
                        let is_same_ident = match &arg_expr.kind {
                            ExprKind::Identifier { name } => name.as_ref() == param_name.as_str(),
                            _ => false,
                        };

                        if !is_same_ident {
                            hints.push(InlayHint {
                                position: Position {
                                    line,
                                    character: col,
                                },
                                label: InlayHintLabel::String(format!("{}: ", param_name)),
                                kind: Some(InlayHintKind::PARAMETER),
                                text_edits: None,
                                tooltip: None,
                                padding_left: Some(false),
                                padding_right: Some(true),
                                data: None,
                            });
                        }
                    }
                }
                collect_param_hints_in_expr(state, arg_expr, hints);
            }
        }
    }

    match &expr.kind {
        ExprKind::Binary { left, right, .. } => {
            collect_param_hints_in_expr(state, left, hints);
            collect_param_hints_in_expr(state, right, hints);
        }
        ExprKind::Unary { operand, .. } => collect_param_hints_in_expr(state, operand, hints),
        ExprKind::Pipeline { left, right } => {
            collect_param_hints_in_expr(state, left, hints);
            collect_param_hints_in_expr(state, right, hints);
        }
        _ => {}
    }
}

fn resolve_callee_params(state: &DocumentState, callee: &Expr) -> Vec<String> {
    // 0. Direct CallResolution from Checker
    if let Some(call_res) = state.db.call_resolutions.get(&callee.range.start.offset) {
        return call_res
            .params
            .iter()
            .filter_map(|p| p.name.as_deref().map(str::to_string))
            .collect();
    }

    // 1. Direct Checker type info on callee node
    if let Some(info) = state.db.expr_types.get(&callee.range.start.offset) {
        if let TypeKind::Fn(ft) = &info.ty.0 {
            return ft
                .params
                .iter()
                .filter_map(|p| p.name.as_deref().map(str::to_string))
                .collect();
        }
    }

    // 2. Lexical scope resolution for identifier callees
    if let ExprKind::Identifier { name } = &callee.kind {
        if let Some((_, ty)) = state.db.resolve_at(name, callee.range.start.offset) {
            if let TypeKind::Fn(ft) = &ty.0 {
                return ft
                    .params
                    .iter()
                    .filter_map(|p| p.name.as_deref().map(str::to_string))
                    .collect();
            }
        }
    }

    Vec::new()
}
