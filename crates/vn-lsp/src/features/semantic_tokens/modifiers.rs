use rustc_hash::FxHashMap;
use vn_core::TokenKind;
use vn_sem::semantic_info::ResolvedMemberKind;
use vn_sem::symbol::{SymbolId, SymbolKind};

use super::{
    MOD_ABSTRACT, MOD_ASYNC, MOD_DECLARATION, MOD_DEFAULT_LIBRARY, MOD_DEPRECATED, MOD_READONLY,
    MOD_STATIC,
};
use crate::document::{DocumentState, TokenRecord};

const STD_MODULE_PREFIX: &str = "std:";

pub struct DeclarationIndex {
    by_offset: FxHashMap<u32, SymbolId>,
    token_of: FxHashMap<SymbolId, usize>,
}

impl DeclarationIndex {
    pub fn build(state: &DocumentState) -> Self {
        let mut by_offset = FxHashMap::default();
        let mut token_of = FxHashMap::default();
        for (id, symbol) in state.db.bind.arena.all().iter().enumerate() {
            if symbol.origin_module.is_some() {
                continue;
            }
            let first = state.tokens.partition_point(|t| t.offset < symbol.offset);
            if let Some(index) = declared_name_token(state, first, state.name(symbol.name)) {
                by_offset.entry(state.tokens[index].offset).or_insert(id);
                token_of.entry(id).or_insert(index);
            }
        }
        Self {
            by_offset,
            token_of,
        }
    }

    pub fn symbol_at(&self, offset: u32) -> Option<SymbolId> {
        self.by_offset.get(&offset).copied()
    }
}

fn declared_name_token(state: &DocumentState, first: usize, name: &str) -> Option<usize> {
    let tokens = &state.tokens;
    let mut i = first;
    while i < tokens.len() {
        let kind = tokens[i].kind;
        if kind == TokenKind::At {
            i += 2;
            if tokens.get(i).map(|t| t.kind) == Some(TokenKind::LParen) {
                i = skip_parens(state, i);
            }
        } else if kind == TokenKind::Identifier {
            return (state.lexeme(&tokens[i]) == name).then_some(i);
        } else if kind.is_keyword() || kind == TokenKind::Dot {
            i += 1;
        } else {
            return None;
        }
    }
    None
}

fn skip_parens(state: &DocumentState, open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, tok) in state.tokens[open..].iter().enumerate() {
        if tok.kind == TokenKind::LParen {
            depth += 1;
        }
        if tok.kind == TokenKind::RParen {
            depth -= 1;
            if depth == 0 {
                return open + offset + 1;
            }
        }
    }
    state.tokens.len()
}

pub fn modifiers_of(
    state: &DocumentState,
    declarations: &DeclarationIndex,
    index: usize,
    tok: &TokenRecord,
) -> u32 {
    if tok.kind == TokenKind::This {
        return MOD_READONLY;
    }

    let mut bits = 0;
    let declared = declarations.symbol_at(tok.offset);
    let referenced = state
        .db
        .expr_types
        .get(&tok.offset)
        .and_then(|info| info.symbol_id)
        .filter(|id| *id < state.db.bind.arena.len());

    if declared.is_some() {
        bits |= MOD_DECLARATION;
        bits |= declaration_modifiers(state, index);
    }

    if let Some(id) = declared.or(referenced) {
        let symbol = state.db.bind.arena.get(id);
        if symbol.kind == SymbolKind::Const {
            bits |= MOD_READONLY;
        }
        if symbol.is_async {
            bits |= MOD_ASYNC;
        }
        if symbol.deprecated.is_some() {
            bits |= MOD_DEPRECATED;
        }
        if symbol
            .origin_module
            .is_some_and(|module| state.name(module).starts_with(STD_MODULE_PREFIX))
        {
            bits |= MOD_DEFAULT_LIBRARY;
        }
        if declared.is_none()
            && let Some(&declaration) = declarations.token_of.get(&id)
        {
            bits |= declaration_modifiers(state, declaration);
        }
    }

    if let Some(resolution) = state.db.member_resolutions.get(&tok.offset)
        && matches!(
            resolution.member_kind,
            ResolvedMemberKind::StaticMethod | ResolvedMemberKind::StaticProperty
        )
    {
        bits |= MOD_STATIC;
    }
    bits
}

fn declaration_modifiers(state: &DocumentState, index: usize) -> u32 {
    let mut bits = 0;
    for tok in state.tokens[..index].iter().rev() {
        if !is_declaration_prefix(tok.kind) {
            break;
        }
        if tok.kind == TokenKind::Static {
            bits |= MOD_STATIC;
        }
        if tok.kind == TokenKind::Abstract {
            bits |= MOD_ABSTRACT;
        }
        if tok.kind == TokenKind::Readonly {
            bits |= MOD_READONLY;
        }
        if tok.kind == TokenKind::Async {
            bits |= MOD_ASYNC;
        }
    }
    bits
}

fn is_declaration_prefix(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Static
            | TokenKind::Abstract
            | TokenKind::Readonly
            | TokenKind::Async
            | TokenKind::Public
            | TokenKind::Private
            | TokenKind::Protected
            | TokenKind::Override
            | TokenKind::Declare
            | TokenKind::Native
            | TokenKind::Export
            | TokenKind::Get
            | TokenKind::Set
            | TokenKind::Function
            | TokenKind::Class
            | TokenKind::Struct
            | TokenKind::Interface
            | TokenKind::Enum
            | TokenKind::Namespace
            | TokenKind::Type
    )
}
