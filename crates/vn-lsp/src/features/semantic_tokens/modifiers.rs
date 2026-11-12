use rustc_hash::FxHashMap;
use vn_core::TokenKind;
use vn_sem::semantic_info::ResolvedMemberKind;
use vn_sem::symbol::{SymbolId, SymbolKind};

use super::{MOD_ABSTRACT, MOD_ASYNC, MOD_DECLARATION, MOD_DEPRECATED, MOD_READONLY, MOD_STATIC};
use crate::document::{DocumentState, TokenRecord};

const NAME_SEARCH_WINDOW: usize = 8;

pub struct DeclarationIndex {
    by_offset: FxHashMap<u32, SymbolId>,
}

impl DeclarationIndex {
    pub fn build(state: &DocumentState) -> Self {
        let mut by_offset = FxHashMap::default();
        for (id, symbol) in state.db.bind.arena.all().iter().enumerate() {
            if symbol.origin_module.is_some() {
                continue;
            }
            let name = state.name(symbol.name);
            let first = state.tokens.partition_point(|t| t.offset < symbol.offset);
            let name_token = state.tokens[first..]
                .iter()
                .take(NAME_SEARCH_WINDOW)
                .find(|t| t.kind == TokenKind::Identifier && state.lexeme(t) == name);
            if let Some(token) = name_token {
                by_offset.entry(token.offset).or_insert(id);
            }
        }
        Self { by_offset }
    }
}

impl DeclarationIndex {
    pub fn symbol_at(&self, offset: u32) -> Option<SymbolId> {
        self.by_offset.get(&offset).copied()
    }
}

pub fn modifiers_of(
    state: &DocumentState,
    declarations: &DeclarationIndex,
    index: usize,
    tok: &TokenRecord,
) -> u32 {
    let mut bits = 0;
    if tok.kind == TokenKind::This {
        return MOD_READONLY;
    }

    let declared = declarations.by_offset.get(&tok.offset).copied();
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
