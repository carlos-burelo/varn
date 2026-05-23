mod classify;

use std::collections::HashMap;

use once_cell::sync::Lazy;
use tower_lsp::lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};
use varn_checker::SymbolKind;
use varn_core::TokenKind;

use crate::document::DocumentState;
use crate::query;

pub static LEGEND: Lazy<SemanticTokensLegend> = Lazy::new(|| SemanticTokensLegend {
    token_types: vec![
        SemanticTokenType::KEYWORD,
        SemanticTokenType::TYPE,
        SemanticTokenType::VARIABLE,
        SemanticTokenType::FUNCTION,
        SemanticTokenType::CLASS,
        SemanticTokenType::PARAMETER,
        SemanticTokenType::PROPERTY,
        SemanticTokenType::NUMBER,
        SemanticTokenType::STRING,
        SemanticTokenType::ENUM_MEMBER,
        SemanticTokenType::NAMESPACE,
        SemanticTokenType::INTERFACE,
        SemanticTokenType::TYPE_PARAMETER,
    ],
    token_modifiers: vec![
        SemanticTokenModifier::DECLARATION,
        SemanticTokenModifier::READONLY,
        SemanticTokenModifier::ASYNC,
        SemanticTokenModifier::STATIC,
        SemanticTokenModifier::ABSTRACT,
    ],
});

pub const TT_KEYWORD: u32 = 0;
pub const TT_TYPE: u32 = 1;
pub const TT_VARIABLE: u32 = 2;
pub const TT_FUNCTION: u32 = 3;
pub const TT_CLASS: u32 = 4;
pub const TT_PARAMETER: u32 = 5;
pub const TT_PROPERTY: u32 = 6;
pub const TT_NUMBER: u32 = 7;
pub const TT_STRING: u32 = 8;
pub const TT_ENUM_MEMBER: u32 = 9;
pub const TT_NAMESPACE: u32 = 10;
pub const TT_INTERFACE: u32 = 11;
pub const TT_TYPE_PARAMETER: u32 = 12;

pub const MOD_DECLARATION: u32 = 1 << 0;
pub const MOD_READONLY: u32 = 1 << 1;
pub const MOD_ASYNC: u32 = 1 << 2;
pub const MOD_STATIC: u32 = 1 << 3;
pub const MOD_ABSTRACT: u32 = 1 << 4;

pub fn build_semantic_tokens(state: &DocumentState) -> Vec<u32> {
    use crate::document::MemberKind;

    let mut member_overrides: HashMap<(u32, String), (u32, u32)> = HashMap::new();
    for sym in &state.symbols {
        if matches!(
            sym.kind,
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
        ) {
            for member in &sym.members {
                if member.line == u32::MAX {
                    continue;
                }
                let tt = match member.kind {
                    MemberKind::Property | MemberKind::Variable | MemberKind::Getter | MemberKind::Setter => TT_PROPERTY,
                    MemberKind::Method
                    | MemberKind::Function => TT_FUNCTION,
                    MemberKind::Constructor => continue,
                    MemberKind::Class | MemberKind::Namespace | MemberKind::Struct => TT_CLASS,
                    MemberKind::Interface => TT_INTERFACE,
                    MemberKind::Enum => TT_ENUM_MEMBER,
                    MemberKind::EnumMember => TT_ENUM_MEMBER,
                };
                let mods = if member.is_static { MOD_STATIC } else { 0 };
                member_overrides.insert((member.line, member.name.clone()), (tt, mods));
            }
        }
    }

    let tokens = &state.tokens;
    let mut result = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;
    let mut paren_depth: i32 = 0;

    for (i, tok) in tokens.iter().enumerate() {
        let next_is_lparen = tokens
            .get(i + 1)
            .map(|t| t.kind == TokenKind::LParen)
            .unwrap_or(false);
        let prev_is_dot = i
            .checked_sub(1)
            .and_then(|j| tokens.get(j))
            .map(|t| t.kind == TokenKind::Dot)
            .unwrap_or(false);
        let next_is_colon = tokens
            .get(i + 1)
            .map(|t| t.kind == TokenKind::Colon)
            .unwrap_or(false);

        let mut token_type: Option<u32> = None;
        let mut modifier: u32 = 0;

        if token_type.is_none() {
            if let Some(&(tt, mods)) = member_overrides.get(&(tok.line, tok.lexeme.clone())) {
                token_type = Some(tt);
                modifier = mods;
            }
        }

        if token_type.is_none() {
            if !matches!(
                tok.kind,
                TokenKind::From | TokenKind::As | TokenKind::Import | TokenKind::Export
            ) {
                if let Some(res) = query::resolve_chain(state, tok.line, tok.col) {
                    token_type = Some(match res {
                        crate::document::ChainResult::Symbol(s) => {
                            if tok.line == s.line && tok.col == s.col {
                                modifier |= MOD_DECLARATION;
                            }
                            match s.kind {
                                SymbolKind::Function | SymbolKind::Method => TT_FUNCTION,
                                SymbolKind::Class | SymbolKind::Struct => TT_CLASS,
                                SymbolKind::Extension => TT_VARIABLE,
                                SymbolKind::Namespace => TT_NAMESPACE,
                                SymbolKind::Interface => TT_INTERFACE,
                                SymbolKind::TypeAlias | SymbolKind::Enum => TT_TYPE,
                                SymbolKind::EnumMember => TT_ENUM_MEMBER,
                                SymbolKind::TypeParameter => TT_TYPE_PARAMETER,
                                SymbolKind::Parameter => {
                                    if matches!(s.ty.0, varn_core::TypeKind::Fn(_)) {
                                        TT_FUNCTION
                                    } else {
                                        TT_PARAMETER
                                    }
                                }
                                SymbolKind::Property => {
                                    if matches!(s.ty.0, varn_core::TypeKind::Fn(_)) {
                                        TT_FUNCTION
                                    } else {
                                        TT_PROPERTY
                                    }
                                }
                                SymbolKind::Const | SymbolKind::Let | SymbolKind::Var => {
                                    if matches!(s.ty.0, varn_core::TypeKind::Fn(_)) {
                                        TT_FUNCTION
                                    } else {
                                        TT_VARIABLE
                                    }
                                }
                            }
                        }
                        crate::document::ChainResult::Member { member, .. } => {
                            classify::map_member_kind_to_tt(&member.kind)
                        }
                        crate::document::ChainResult::DynamicMember { member, .. } => {
                            classify::map_member_kind_to_tt(&member.kind)
                        }
                    });
                }
            }
        }

        if token_type.is_none() {
            if let Some((_, _, m)) = query::member_at(state, tok.line, tok.col) {
                token_type = Some(classify::map_member_kind_to_tt(&m.kind));
            }
        }

        if token_type.is_none() {
            token_type = if tok.kind == TokenKind::Identifier
                && !prev_is_dot
                && !next_is_lparen
                && paren_depth > 0
                && next_is_colon
            {
                if let Some((_, ty)) = state.db.resolve_at(&tok.lexeme, tok.offset) {
                    if matches!(ty.0, varn_core::TypeKind::Fn(_)) {
                        Some(TT_FUNCTION)
                    } else {
                        Some(TT_PARAMETER)
                    }
                } else {
                    Some(TT_PARAMETER)
                }
            } else if tok.kind == TokenKind::Identifier
                && !prev_is_dot
                && !next_is_lparen
                && paren_depth == 0
                && next_is_colon
            {
                Some(TT_PROPERTY)
            } else if tok.kind == TokenKind::Identifier
                && !prev_is_dot
                && !next_is_lparen
                && paren_depth == 0
                && classify::param_scope_type(tok, &state.param_scopes)
            {
                if let Some((_, ty)) = state.db.resolve_at(&tok.lexeme, tok.offset) {
                    if matches!(ty.0, varn_core::TypeKind::Fn(_)) {
                        Some(TT_FUNCTION)
                    } else {
                        Some(TT_PARAMETER)
                    }
                } else {
                    Some(TT_PARAMETER)
                }
            } else {
                classify::classify(
                    tok,
                    &state.symbol_map,
                    next_is_lparen,
                    prev_is_dot,
                    &state.type_param_names,
                )
            };
        }

        match tok.kind {
            TokenKind::LParen => paren_depth += 1,
            TokenKind::RParen if paren_depth > 0 => paren_depth -= 1,
            _ => {}
        }

        if token_type.is_some()
            && (tok.kind != TokenKind::Identifier
                && !tok.kind.is_keyword()
                && !tok.kind.is_literal()
                && !matches!(
                    tok.kind,
                    TokenKind::Arrow | TokenKind::FatArrow | TokenKind::PipeGt
                ))
        {
            token_type = None;
        }

        let Some(token_type) = token_type else {
            continue;
        };

        let (emit_col, emit_len) = match tok.kind {
            TokenKind::TemplateHead => (tok.col, tok.length.saturating_sub(2)),
            TokenKind::TemplateMiddle => (tok.col + 1, tok.length.saturating_sub(3)),
            TokenKind::TemplateTail => (tok.col + 1, tok.length.saturating_sub(1)),
            _ => (tok.col, tok.length),
        };

        if emit_len == 0 {
            continue;
        }

        let base_modifier: u32 = match tok.kind {
            TokenKind::This => MOD_READONLY,
            TokenKind::Identifier => match state.symbol_map.get(tok.lexeme.as_str()) {
                Some(SymbolKind::Const | SymbolKind::Extension) => MOD_READONLY,
                _ => 0,
            },
            _ => 0,
        };
        let combined_modifier = modifier | base_modifier;

        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            emit_col - prev_col
        } else {
            emit_col
        };

        result.push(delta_line);
        result.push(delta_start);
        result.push(emit_len);
        result.push(token_type);
        result.push(combined_modifier);

        prev_line = tok.line;
        prev_col = emit_col;
    }

    result
}
