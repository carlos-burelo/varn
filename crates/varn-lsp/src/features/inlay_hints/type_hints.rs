use crate::document::SymbolView;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use varn_checker::SymbolKind;
use varn_core::ast::{Expr, ExprKind, Program, Stmt, StmtKind};
use varn_core::TypeKind;

use crate::document::DocumentState;

pub fn build_type_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for s in state.symbols() {
        if s.line() == u32::MAX || s.is_from_stdlib() {
            continue;
        }

        match s.kind() {
            SymbolKind::Const | SymbolKind::Let | SymbolKind::Var
                if !s.has_explicit_type() && !s.type_str().is_empty() =>
            {
                let hint_col = s.col() + s.name().len() as u32;
                hints.push(InlayHint {
                    position: Position {
                        line: s.line(),
                        character: hint_col,
                    },
                    label: InlayHintLabel::String(format!(": {}", s.type_str())),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false),
                    padding_right: Some(true),
                    data: None,
                });
            }

            SymbolKind::Function | SymbolKind::Method => {
                if let Some(hint) = fn_return_hint(state, s) {
                    hints.push(hint);
                }
            }

            _ => {}
        }
    }

    if let Some(program) = &state.ast {
        collect_pipeline_hints(state, program, &mut hints);
    }

    hints
}

fn fn_return_hint(state: &DocumentState, sym: SymbolView<'_>) -> Option<InlayHint> {
    if sym.has_explicit_type() {
        return None;
    }

    let ret_ty = match &sym.ty().0 {
        TypeKind::Fn(ft) => ft.return_type.as_ref(),
        _ => return None,
    };

    if let TypeKind::Intrinsic(varn_core::TypeTag::Void | varn_core::TypeTag::Dynamic) = &ret_ty.0 {
        return None;
    }
    let ret_str = ret_ty.to_string();
    if ret_str.is_empty() || ret_str == "unknown" || ret_str == "void" {
        return None;
    }

    let rparen_col = find_rparen_col_on_line(state, sym.line(), sym.col())?;

    Some(InlayHint {
        position: Position {
            line: sym.line(),
            character: rparen_col + 1,
        },
        label: InlayHintLabel::String(format!(": {ret_str}")),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(true),
        data: None,
    })
}

fn find_rparen_col_on_line(state: &DocumentState, line: u32, after_col: u32) -> Option<u32> {
    let mut depth = 0i32;
    let mut last_rparen_col = None;
    for tok in state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col >= after_col)
    {
        match tok.kind {
            varn_core::TokenKind::LParen => depth += 1,
            varn_core::TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    last_rparen_col = Some(tok.col + tok.length.saturating_sub(1));
                    break;
                }
            }
            _ => {}
        }
    }
    last_rparen_col
}

fn collect_pipeline_hints(state: &DocumentState, program: &Program, hints: &mut Vec<InlayHint>) {
    for stmt in &program.body {
        collect_pipeline_in_stmt(state, stmt, hints);
    }
}

fn collect_pipeline_in_stmt(state: &DocumentState, stmt: &Stmt, hints: &mut Vec<InlayHint>) {
    match &stmt.kind {
        StmtKind::Expr { expression } => collect_pipeline_in_expr(state, expression, hints),
        StmtKind::Block { stmts } => {
            for s in stmts {
                collect_pipeline_in_stmt(state, s, hints);
            }
        }
        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            collect_pipeline_in_expr(state, test, hints);
            collect_pipeline_in_stmt(state, consequent, hints);
            if let Some(alt) = alternate {
                collect_pipeline_in_stmt(state, alt, hints);
            }
        }
        _ => {}
    }
}

fn collect_pipeline_in_expr(state: &DocumentState, expr: &Expr, hints: &mut Vec<InlayHint>) {
    if let ExprKind::Pipeline { left, right } = &expr.kind {
        if let Some(info) = state.db.expr_types.get(&right.id) {
            let ty_str = info.ty.to_string();
            if !ty_str.is_empty() && ty_str != "unknown" && ty_str != "void" {
                let r_end = &right.range.end;
                hints.push(InlayHint {
                    position: Position {
                        line: r_end.line.saturating_sub(1),
                        character: r_end.column,
                    },
                    label: InlayHintLabel::String(format!(": {ty_str}")),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: Some(false),
                    data: None,
                });
            }
        }
        collect_pipeline_in_expr(state, left, hints);
        collect_pipeline_in_expr(state, right, hints);
    }
}
