use vn_core::TokenKind;
use vn_sem::resolver::ImportResolver;
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
    pub function_type_parameter: bool,
    pub enum_member_declaration: bool,
    pub declared: Option<SymbolId>,
    pub specifier_kind: Option<SymbolKind>,
    pub prev_is_colon_colon: bool,
    pub pattern: Option<super::roles::PatternRole>,
    pub member: Option<super::roles::MemberRole>,
}

impl TokenContext {
    pub fn of(state: &DocumentState, declarations: &DeclarationIndex, index: usize) -> Self {
        let tokens = &state.tokens;
        let tok = &tokens[index];
        let kind_at = |i: Option<usize>| i.and_then(|i| tokens.get(i)).map(|t| t.kind);
        let prev = kind_at(index.checked_sub(1));
        let next = kind_at(Some(index + 1));
        let prev_is_dot = prev == Some(TokenKind::Dot);

        let in_specifier_list = in_specifier_list(state, index);
        let specifier_kind =
            if in_specifier_list && !state.symbol_map.contains_key(state.lexeme(tok)) {
                exported_kind(state, index)
            } else {
                None
            };

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
            in_specifier_list,
            function_type_parameter: function_type_parameter(state, index),
            enum_member_declaration: declarations.is_enum_member(tok.offset),
            declared: declarations.symbol_at(tok.offset),
            specifier_kind,
            prev_is_colon_colon: prev == Some(TokenKind::ColonColon),
            pattern: declarations.pattern_role(tok.offset),
            member: declarations.member_role(tok.offset),
        }
    }
}

fn function_type_parameter(state: &DocumentState, index: usize) -> bool {
    let tokens = &state.tokens;
    let labelled = |i: usize| {
        let kind_at = |j: usize| tokens.get(j).map(|t| t.kind);
        kind_at(i + 1) == Some(TokenKind::Colon)
            || (kind_at(i + 1) == Some(TokenKind::Question)
                && kind_at(i + 2) == Some(TokenKind::Colon))
    };
    let after_open_or_comma = index
        .checked_sub(1)
        .and_then(|j| tokens.get(j))
        .is_some_and(|t| matches!(t.kind, TokenKind::LParen | TokenKind::Comma));
    if tokens[index].kind != TokenKind::Identifier || !labelled(index) || !after_open_or_comma {
        return false;
    }

    let mut depth = 0usize;
    let mut open = None;
    for j in (0..index).rev() {
        let kind = tokens[j].kind;
        if matches!(
            kind,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        ) {
            depth += 1;
        } else if matches!(
            kind,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
        ) {
            if depth > 0 {
                depth -= 1;
            } else if kind == TokenKind::LParen {
                open = Some(j);
                break;
            } else {
                return false;
            }
        }
    }
    let Some(open) = open else {
        return false;
    };

    let mut depth = 0usize;
    for (offset, tok) in tokens[open..].iter().enumerate() {
        if tok.kind == TokenKind::LParen {
            depth += 1;
        }
        if tok.kind == TokenKind::RParen {
            depth -= 1;
            if depth == 0 {
                return matches!(
                    tokens.get(open + offset + 1).map(|t| t.kind),
                    Some(TokenKind::FatArrow | TokenKind::Arrow)
                );
            }
        }
    }
    false
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

fn exported_kind(state: &DocumentState, index: usize) -> Option<SymbolKind> {
    let tokens = &state.tokens;
    let close = tokens[index..]
        .iter()
        .position(|t| t.kind == TokenKind::RBrace)?
        + index;
    let from = tokens
        .get(close + 1)
        .filter(|t| t.kind == TokenKind::From)?;
    let literal = tokens
        .get(close + 2)
        .filter(|t| t.kind == TokenKind::Str && t.offset > from.offset)?;
    let specifier = state.lexeme(literal).trim_matches(['"', '\'']);
    let name = state.lexeme(&tokens[index]);
    let exports = if specifier.starts_with("std:") {
        state.resolver.stdlib_exports(specifier)
    } else {
        let path = crate::document::uri_to_path(&state.uri);
        let base = std::path::Path::new(&path).parent()?;
        let resolved = state.resolver.resolve_specifier(base, specifier)?;
        state.resolver.module_exports(&resolved, &mut Vec::new())
    };
    exports.get(name).map(|symbol| symbol.kind)
}
