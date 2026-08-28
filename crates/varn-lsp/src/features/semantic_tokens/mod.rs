mod classify;

use once_cell::sync::Lazy;
use tower_lsp::lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};
use varn_checker::SymbolKind;
use varn_core::TokenKind;

use crate::document::DocumentState;

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

/// Build the LSP semantic-token stream. Every identifier-bearing token is
/// classified by [`classify::resolve_token`], which is driven entirely by the
/// checker (`expr_types` + lexical `resolve_at`); there are no token-scanned
/// heuristics here anymore.
pub fn build_semantic_tokens(state: &DocumentState) -> Vec<u32> {
    let tokens = &state.tokens;
    let mut result = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    for (i, tok) in tokens.iter().enumerate() {
        // Only real source tokens carry colour.
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
        // `name:` — object-literal key position. (Declared fields/params, incl.
        // optional `name?:`, resolve via their recorded symbol, so this only
        // needs to catch unrecorded object-literal keys.)
        let next_is_colon = tokens
            .get(i + 1)
            .map(|t| t.kind == TokenKind::Colon)
            .unwrap_or(false);

        // Receiver of a `recv.member` access is an enum *type* — marks the
        // member as an enum variant (`Shape.Circle`), distinct from a field
        // access on an enum *value* (`ok.code`). Sourced from the checker's
        // symbol kinds, not a token scan.
        let prev2_is_enum = prev_is_dot
            && i >= 2
            && matches!(
                state.symbol_map.get(tokens[i - 2].lexeme.as_str()),
                Some(SymbolKind::Enum)
            );

        // `get`/`set` are keywords only as accessor declarations (`get name()`),
        // i.e. immediately followed by the accessor name. Everywhere else they
        // are ordinary identifiers and must be resolved as such.
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

        // `readonly` modifier for `this` and `const` bindings — derived from the
        // checker's symbol kind, not a name table.
        let modifier = if tok.kind == TokenKind::This
            || state
                .db
                .expr_types
                .get(&tok.offset)
                .and_then(|info| info.symbol_id)
                .filter(|s| *s < state.db.arena.len())
                .map(|s| state.db.arena.get(s).kind)
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
