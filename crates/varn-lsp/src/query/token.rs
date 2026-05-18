use varn_core::TokenKind;

use crate::document::{DocumentState, TokenRecord};

fn is_expression_keyword(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Await | TokenKind::Yield)
}

fn is_after_dot(state: &DocumentState, idx: usize) -> bool {
    idx.checked_sub(1)
        .and_then(|j| state.tokens.get(j))
        .is_some_and(|p| p.kind == TokenKind::Dot || p.kind == TokenKind::QuestionDot)
}

pub fn token_at(state: &DocumentState, line: u32, col: u32) -> Option<&TokenRecord> {
    let idx = token_index_at(state, line, col)?;
    Some(&state.tokens[idx])
}

pub fn token_index_at(state: &DocumentState, line: u32, col: u32) -> Option<usize> {
    let idx = state.tokens.iter().position(|t| {
        t.line == line
            && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
            && t.col <= col
            && col < t.col + t.length
    })?;

    if is_expression_keyword(state.tokens[idx].kind) && !is_after_dot(state, idx) {
        return None;
    }

    Some(idx)
}

pub fn prev_token(state: &DocumentState, idx: usize) -> Option<(usize, &TokenRecord)> {
    idx.checked_sub(1)
        .and_then(|j| state.tokens.get(j).map(|t| (j, t)))
}

pub fn next_token(state: &DocumentState, idx: usize) -> Option<(usize, &TokenRecord)> {
    let j = idx + 1;
    state.tokens.get(j).map(|t| (j, t))
}
