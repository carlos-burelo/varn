use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use varn_core::ast::{Arg, ExprId, ExprKind};

use crate::document::DocumentState;

/// A hint naming the parameter each argument of a call binds, for every call
/// in the document.
pub fn build_parameter_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for expr in state.spatial_index.exprs() {
        if let ExprKind::Call { callee, args, .. } = &state.ast_arena.expr(expr).kind {
            if !args.is_empty() {
                call_hints(state, *callee, args, &mut hints);
            }
        }
    }
    hints
}

fn call_hints(state: &DocumentState, callee: ExprId, args: &[Arg], hints: &mut Vec<InlayHint>) {
    let arena = &state.ast_arena;
    let param_names = resolve_callee_params(state, callee);
    for (arg, param_name) in args.iter().zip(&param_names) {
        let arg_expr = match arg {
            // A named argument already says which parameter it binds; a
            // spread binds several.
            Arg::Named { .. } | Arg::Spread(_) => continue,
            Arg::Positional(e) => arena.expr(*e),
        };
        if param_name.is_empty() || param_name.starts_with('_') {
            continue;
        }
        // An argument that is already the parameter's own name says it.
        if let ExprKind::Identifier { name } = &arg_expr.kind {
            if state.name(*name) == param_name {
                continue;
            }
        }
        let arg_start = &arg_expr.range.start;
        hints.push(InlayHint {
            position: Position {
                line: arg_start.line.saturating_sub(1),
                character: arg_start.column,
            },
            label: InlayHintLabel::String(format!("{param_name}: ")),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: Some(false),
            padding_right: Some(true),
            data: None,
        });
    }
}

/// The parameter names of the function `callee` calls, in order.
fn resolve_callee_params(state: &DocumentState, callee: ExprId) -> Vec<String> {
    let node = state.ast_arena.expr(callee);
    let names = |f: varn_checker::types::FunctionType| {
        f.params
            .iter()
            .map(|p| p.name.as_deref().unwrap_or_default().to_owned())
            .collect()
    };

    // The checker's own resolution of the call.
    if let Some(call_res) = state.db.call_resolutions.get(&node.range.start.offset) {
        return call_res
            .params
            .iter()
            .map(|p| p.name.as_deref().unwrap_or_default().to_owned())
            .collect();
    }

    // The type the checker gave the callee.
    if let Some(entry) = state.db.expr_table.get(&callee.index()) {
        if let Some(f) = state.db.callable_shape(&entry.ty) {
            return names(f);
        }
    }

    // A name, resolved in its scope.
    if let ExprKind::Identifier { name } = &node.kind {
        if let Some((_, ty)) = state
            .db
            .resolve_at(state.name(*name), node.range.start.offset)
        {
            if let Some(f) = state.db.fn_shape(&ty) {
                return names(f);
            }
        }
    }

    Vec::new()
}
