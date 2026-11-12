use rustc_hash::{FxHashMap, FxHashSet};
use vn_core::{TokenKind, TypeKind};
use vn_sem::semantic_info::ResolvedMemberKind;
use vn_sem::symbol::{SymbolId, SymbolKind};

use super::{
    MOD_ABSTRACT, MOD_ASYNC, MOD_DECLARATION, MOD_DEFAULT_LIBRARY, MOD_DEPRECATED, MOD_READONLY,
    MOD_STATIC,
};
use crate::document::{DocumentState, TokenRecord};

const LIBRARY_MODULE_PREFIXES: [&str; 2] = ["std:", "core:"];

pub struct DeclarationIndex {
    by_offset: FxHashMap<u32, SymbolId>,
    token_of: FxHashMap<SymbolId, usize>,
    enum_members: FxHashSet<u32>,
    imported: FxHashSet<String>,
    roles: super::roles::Roles,
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
            enum_members: enum_member_offsets(state),
            imported: imported_names(state),
            roles: super::roles::Roles::of(state),
        }
    }

    pub fn pattern_role(&self, offset: u32) -> Option<super::roles::PatternRole> {
        self.roles.patterns.get(&offset).copied()
    }

    pub fn member_role(&self, offset: u32) -> Option<super::roles::MemberRole> {
        self.roles.members.get(&offset).copied()
    }

    pub fn is_imported(&self, name: &str) -> bool {
        self.imported.contains(name)
    }

    pub fn is_enum_member(&self, offset: u32) -> bool {
        self.enum_members.contains(&offset)
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
        } else if is_word(kind) && state.lexeme(&tokens[i]) == name {
            return Some(i);
        } else if is_word(kind) || kind == TokenKind::Dot {
            i += 1;
        } else {
            return None;
        }
    }
    None
}

fn enum_member_offsets(state: &DocumentState) -> FxHashSet<u32> {
    let tokens = &state.tokens;
    let mut members = FxHashSet::default();
    for (start, tok) in tokens.iter().enumerate() {
        if tok.kind != TokenKind::Enum {
            continue;
        }
        let Some(open) = tokens[start..]
            .iter()
            .take_while(|t| t.kind != TokenKind::Semicolon)
            .position(|t| t.kind == TokenKind::LBrace)
            .map(|p| start + p)
        else {
            continue;
        };
        let mut depth = 0usize;
        for i in open + 1..tokens.len() {
            let kind = tokens[i].kind;
            if matches!(
                kind,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
            ) {
                depth += 1;
            } else if kind == TokenKind::RBrace && depth == 0 {
                break;
            } else if matches!(
                kind,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
            ) {
                depth = depth.saturating_sub(1);
            } else if kind == TokenKind::Identifier
                && depth == 0
                && matches!(
                    tokens[i - 1].kind,
                    TokenKind::LBrace | TokenKind::Comma | TokenKind::Semicolon
                )
            {
                members.insert(tokens[i].offset);
            }
        }
    }
    members
}

fn imported_names(state: &DocumentState) -> FxHashSet<String> {
    let tokens = &state.tokens;
    let mut names = FxHashSet::default();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Import {
            continue;
        }
        let Some(open) = tokens
            .get(index + 1)
            .filter(|t| t.kind == TokenKind::LBrace)
        else {
            continue;
        };
        let open_at = open.offset;
        let mut keep_next = true;
        for specifier in tokens.iter().filter(|t| t.offset > open_at) {
            if specifier.kind == TokenKind::RBrace {
                break;
            }
            if specifier.kind == TokenKind::As {
                keep_next = true;
                continue;
            }
            if specifier.kind == TokenKind::Comma {
                keep_next = true;
                continue;
            }
            if specifier.kind == TokenKind::Identifier && keep_next {
                names.insert(state.lexeme(specifier).to_owned());
            }
        }
    }
    names
}

fn is_word(kind: TokenKind) -> bool {
    kind == TokenKind::Identifier
        || kind.is_keyword()
        || matches!(kind, TokenKind::Mut | TokenKind::Out)
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
    if declared.is_none() {
        if declarations.pattern_role(tok.offset) == Some(super::roles::PatternRole::Binding) {
            return MOD_DECLARATION;
        }
        if declarations.member_role(tok.offset).is_some() {
            return MOD_DECLARATION | declaration_modifiers(state, index);
        }
    }
    if declarations.is_enum_member(tok.offset) {
        return MOD_DECLARATION;
    }
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
        let from_std = symbol.line == u32::MAX
            || symbol.origin_module.is_some_and(|module| {
                LIBRARY_MODULE_PREFIXES
                    .iter()
                    .any(|prefix| state.name(module).starts_with(prefix))
            });
        let global_function =
            symbol.kind == SymbolKind::Function && !declarations.is_imported(state.lexeme(tok));
        if from_std && (global_function || symbol.kind == SymbolKind::EnumMember) {
            bits |= MOD_DEFAULT_LIBRARY;
        }
        if declared.is_none()
            && let Some(&declaration) = declarations.token_of.get(&id)
        {
            bits |= declaration_modifiers(state, declaration);
        }
    }

    if let Some(resolution) = state.db.member_resolutions.get(&tok.offset)
        && resolution.member_kind == ResolvedMemberKind::EnumMember
        && let TypeKind::Named(_, Some(origin)) | TypeKind::Generic(_, _, Some(origin)) =
            state.db.ty_kind(&resolution.receiver_ty)
        && LIBRARY_MODULE_PREFIXES
            .iter()
            .any(|prefix| state.name(origin).starts_with(prefix))
    {
        bits |= MOD_DEFAULT_LIBRARY;
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
