use varn_core::TokenKind;

use super::DocumentState;

impl DocumentState {
    
    
    
    
    
    pub fn type_param_at_pos(&self, line: u32, col: u32) -> Option<String> {
        let tok = self.tokens.iter().find(|t| {
            t.line == line
                && t.kind == TokenKind::Identifier
                && t.col <= col
                && col < t.col + t.length
        })?;
        if self.type_param_names.contains(self.lexeme(tok)) {
            Some(self.lexeme(tok).to_owned())
        } else {
            None
        }
    }
}
