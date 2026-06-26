use varn_core::TokenKind;

use super::{DocumentState, SymbolRecord};

fn is_expression_keyword(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Await | TokenKind::Yield)
}

fn is_identifier_like_for_hover(tokens: &[super::TokenRecord], idx: usize) -> bool {
    let tok = &tokens[idx];
    if !(tok.kind == TokenKind::Identifier || tok.kind.can_be_identifier()) {
        return false;
    }
    if !is_expression_keyword(tok.kind) {
        return true;
    }

    idx.checked_sub(1)
        .and_then(|j| tokens.get(j))
        .is_some_and(|prev| prev.kind == TokenKind::Dot || prev.kind == TokenKind::QuestionDot)
}

impl DocumentState {
    pub fn symbol_at_pos(&self, line: u32, col: u32) -> Option<&SymbolRecord> {
        let tok = self
            .tokens
            .iter()
            .enumerate()
            .find(|(idx, t)| {
                t.line == line
                    && is_identifier_like_for_hover(&self.tokens, *idx)
                    && t.col <= col
                    && col < t.col + t.length
            })
            .map(|(_, t)| t)?;

        if self.member_at_pos(line, col).is_some() {
            return None;
        }

        self.checker_symbol_at(line, col)
            .filter(|sym| sym.name == tok.lexeme)
    }

    /// Type-parameter references inside type annotations are not recorded
    /// per-offset by the checker, so they cannot be resolved through
    /// `expr_types`/`resolve_at`. The name set is built from the checker's
    /// `TypeParameter` symbols (not a token scan), so this stays a name lookup
    /// only until type-node references gain their own recorded entries.
    pub fn type_param_at_pos(&self, line: u32, col: u32) -> Option<String> {
        let tok = self.tokens.iter().find(|t| {
            t.line == line
                && t.kind == TokenKind::Identifier
                && t.col <= col
                && col < t.col + t.length
        })?;
        if self.type_param_names.contains(tok.lexeme.as_str()) {
            Some(tok.lexeme.clone())
        } else {
            None
        }
    }
}
