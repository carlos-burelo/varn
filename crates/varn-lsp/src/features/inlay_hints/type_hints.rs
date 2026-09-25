use crate::document::SymbolView;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use varn_checker::SymbolKind;
use varn_core::ast::ExprKind;
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

    collect_pipeline_hints(state, &mut hints);

    hints
}

fn fn_return_hint(state: &DocumentState, sym: SymbolView<'_>) -> Option<InlayHint> {
    if sym.has_explicit_type() {
        return None;
    }

    let ret_ty = varn_checker::Type(state.db.fn_shape(sym.ty())?.return_type, false);
    if !worth_hinting(state, &ret_ty) {
        return None;
    }
    let ret_str = state.ty_text(&ret_ty);

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

/// Whether a hint showing `ty` tells the reader anything: `dynamic` and
/// `void` do not.
fn worth_hinting(state: &DocumentState, ty: &varn_checker::Type) -> bool {
    !matches!(
        state.db.ty_kind(ty),
        TypeKind::Primitive(varn_core::LangPrimitive::Void | varn_core::LangPrimitive::Dynamic)
    )
}

/// A hint after each pipeline stage: the type of the value it produces.
fn collect_pipeline_hints(state: &DocumentState, hints: &mut Vec<InlayHint>) {
    let arena = &state.ast_arena;
    for expr in state.spatial_index.exprs() {
        let ExprKind::Pipeline { right, .. } = &arena.expr(expr).kind else {
            continue;
        };
        let Some(entry) = state.db.expr_table.get(&expr.index()) else {
            continue;
        };
        if !worth_hinting(state, &entry.ty) {
            continue;
        }
        let r_end = &arena.expr(*right).range.end;
        hints.push(InlayHint {
            position: Position {
                line: r_end.line.saturating_sub(1),
                character: r_end.column,
            },
            label: InlayHintLabel::String(format!(": {}", state.ty_text(&entry.ty))),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: Some(true),
            padding_right: Some(false),
            data: None,
        });
    }
}
