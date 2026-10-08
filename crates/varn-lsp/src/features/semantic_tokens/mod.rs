mod classify;

use once_cell::sync::Lazy;
use tower_lsp_f::lsp_types::{SemanticTokenModifiers, SemanticTokenTypes, SemanticTokensLegend};
use varn_checker::SymbolKind;
use varn_core::TokenKind;

use crate::document::DocumentState;

pub static LEGEND: Lazy<SemanticTokensLegend> = Lazy::new(|| SemanticTokensLegend {
    token_types: vec![
        SemanticTokenTypes::Keyword.into(),
        SemanticTokenTypes::Type.into(),
        SemanticTokenTypes::Variable.into(),
        SemanticTokenTypes::Function.into(),
        SemanticTokenTypes::Class.into(),
        SemanticTokenTypes::Parameter.into(),
        SemanticTokenTypes::Property.into(),
        SemanticTokenTypes::Number.into(),
        SemanticTokenTypes::String.into(),
        SemanticTokenTypes::EnumMember.into(),
        SemanticTokenTypes::Namespace.into(),
        SemanticTokenTypes::Interface.into(),
        SemanticTokenTypes::TypeParameter.into(),
    ],
    token_modifiers: vec![
        SemanticTokenModifiers::Declaration.into(),
        SemanticTokenModifiers::Readonly.into(),
        SemanticTokenModifiers::Async.into(),
        SemanticTokenModifiers::Static.into(),
        SemanticTokenModifiers::Abstract.into(),
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
    let tokens = &state.tokens;
    let mut result = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    for (i, tok) in tokens.iter().enumerate() {
        let colorable = tok.kind == TokenKind::Identifier
            || tok.kind.is_keyword()
            || tok.kind.is_literal()
            || matches!(
                tok.kind,
                TokenKind::Arrow | TokenKind::FatArrow | TokenKind::PipeGt
            );
        if !colorable {
            continue;
        }

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

        let prev2_is_enum = prev_is_dot
            && i >= 2
            && matches!(
                state.symbol_map.get(state.lexeme(&tokens[i - 2])),
                Some(SymbolKind::Enum)
            );

        let getset_as_ident = matches!(tok.kind, TokenKind::Get | TokenKind::Set)
            && !prev_is_dot
            && tokens
                .get(i + 1)
                .map(|t| t.kind != TokenKind::Identifier)
                .unwrap_or(true);

        let Some(token_type) = classify::resolve_token(
            state,
            tok,
            prev_is_dot,
            prev2_is_enum,
            next_is_lparen,
            next_is_colon,
            getset_as_ident,
        ) else {
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

        let modifier = if tok.kind == TokenKind::This
            || state
                .db
                .expr_types
                .get(&tok.offset)
                .and_then(|info| info.symbol_id)
                .filter(|s| *s < state.db.bind.arena.len())
                .map(|s| state.db.bind.arena.get(s).kind)
                == Some(SymbolKind::Const)
        {
            MOD_READONLY
        } else {
            0
        };

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
        result.push(modifier);

        prev_line = tok.line;
        prev_col = emit_col;
    }

    result
}
