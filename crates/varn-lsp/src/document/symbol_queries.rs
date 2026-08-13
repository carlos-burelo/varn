use varn_core::TokenKind;

use super::DocumentState;

impl DocumentState {
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
