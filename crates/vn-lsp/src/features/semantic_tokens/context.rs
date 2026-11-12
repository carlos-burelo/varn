use vn_core::TokenKind;
use vn_sem::symbol::{SymbolId, SymbolKind};

use super::modifiers::DeclarationIndex;
use crate::document::DocumentState;

pub struct TokenContext {
    pub prev_is_dot: bool,
    pub prev_is_at: bool,
    pub prev2_is_enum: bool,
    pub next_is_lparen: bool,
    pub next_is_colon: bool,
    pub getset_as_ident: bool,
    pub in_specifier_list: bool,
    pub enum_member_declaration: bool,
    pub declared: Option<SymbolId>,
}

impl TokenContext {
    pub fn of(state: &DocumentState, declarations: &DeclarationIndex, index: usize) -> Self {
        let tokens = &state.tokens;
        let tok = &tokens[index];
        let kind_at = |i: Option<usize>| i.and_then(|i| tokens.get(i)).map(|t| t.kind);
        let prev = kind_at(index.checked_sub(1));
        let next = kind_at(Some(index + 1));
        let prev_is_dot = prev == Some(TokenKind::Dot);

        Self {
            prev_is_dot,
            prev_is_at: prev == Some(TokenKind::At),
            prev2_is_enum: prev_is_dot
                && index >= 2
                && matches!(
                    state.symbol_map.get(state.lexeme(&tokens[index - 2])),
                    Some(SymbolKind::Enum)
                ),
            next_is_lparen: next == Some(TokenKind::LParen),
            next_is_colon: next == Some(TokenKind::Colon)
                || (next == Some(TokenKind::Question)
                    && kind_at(Some(index + 2)) == Some(TokenKind::Colon)),
            getset_as_ident: matches!(tok.kind, TokenKind::Get | TokenKind::Set)
                && !prev_is_dot
                && next != Some(TokenKind::Identifier),
            in_specifier_list: in_specifier_list(state, index),
            enum_member_declaration: declarations.is_enum_member(tok.offset),
            declared: declarations.symbol_at(tok.offset),
        }
    }
}

fn in_specifier_list(state: &DocumentState, index: usize) -> bool {
    let tokens = &state.tokens;
    for j in (0..index).rev() {
        let kind = tokens[j].kind;
        if matches!(
            kind,
            TokenKind::Identifier | TokenKind::Comma | TokenKind::As | TokenKind::Type
        ) {
            continue;
        }
        if kind != TokenKind::LBrace {
            return false;
        }
        let kind_before = |back: usize| {
            j.checked_sub(back)
                .and_then(|k| tokens.get(k))
                .map(|t| t.kind)
        };
        let before = match kind_before(1) {
            Some(TokenKind::Type) => kind_before(2),
            other => other,
        };
        return matches!(before, Some(TokenKind::Import | TokenKind::Export));
    }
    false
}
